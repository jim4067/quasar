use {
    super::super::resolve::{FieldSemantics, FieldShape, LifecycleConstraint, ReallocConstraint},
    quote::quote,
};

pub(super) fn emit_epilogue(semantics: &[FieldSemantics]) -> syn::Result<proc_macro2::TokenStream> {
    let mut sweep_stmts = Vec::new();
    let mut close_stmts = Vec::new();
    let mut migration_finish_stmts = Vec::new();

    for sem in semantics {
        let field = &sem.core.ident;

        // Migration<From, To> field type: emit finish() call.
        if matches!(sem.core.shape, FieldShape::Migration) {
            let payer = sem.support.payer.clone().ok_or_else(|| {
                syn::Error::new(field.span(), "Migration<From, To> requires a payer field")
            })?;
            migration_finish_stmts.push(quote! {
                self.#field.finish(
                    self.#payer.to_account_view(),
                    &__shared_rent,
                )?;
            });
        }

        let ty = &sem.core.effective_ty;
        for lifecycle in &sem.lifecycle {
            match lifecycle {
                LifecycleConstraint::Sweep { receiver } => {
                    let authority = token_authority(sem).cloned().ok_or_else(|| {
                        syn::Error::new(field.span(), "sweep requires token::authority")
                    })?;
                    let mint = token_mint(sem).cloned().ok_or_else(|| {
                        syn::Error::new(field.span(), "sweep requires token::mint")
                    })?;
                    let token_program = token_program(sem).ok_or_else(|| {
                        syn::Error::new(field.span(), "sweep requires a token program field")
                    })?;
                    sweep_stmts.push(quote! {
                        {
                            type __Target = <#ty as quasar_lang::account_load::AccountLoad>::BehaviorTarget;
                            <__Target as quasar_lang::account_exit::AccountExit>::sweep(
                                self.#field.to_account_view(),
                                quasar_lang::account_exit::SweepCtx {
                                    receiver: self.#receiver.to_account_view(),
                                    mint: self.#mint.to_account_view(),
                                    authority: self.#authority.to_account_view(),
                                    token_program: self.#token_program.to_account_view(),
                                },
                            )?;
                        }
                    });
                }
                LifecycleConstraint::Close { destination } => {
                    let (authority_expr, tp_expr) = if let (Some(authority), Some(token_program)) =
                        (token_authority(sem).cloned(), token_program(sem))
                    {
                        (
                            quote! { Some(self.#authority.to_account_view()) },
                            quote! { Some(self.#token_program.to_account_view()) },
                        )
                    } else {
                        (quote! { None }, quote! { None })
                    };
                    close_stmts.push(quote! {
                        {
                            type __Target = <#ty as quasar_lang::account_load::AccountLoad>::BehaviorTarget;
                            let __view = unsafe {
                                <#ty as quasar_lang::account_load::AccountLoad>::to_account_view_mut(
                                    &mut self.#field,
                                )
                            };
                            <__Target as quasar_lang::account_exit::AccountExit>::close(
                                __view,
                                quasar_lang::account_exit::CloseCtx {
                                    destination: self.#destination.to_account_view(),
                                    authority: #authority_expr,
                                    token_program: #tp_expr,
                                },
                            )?;
                        }
                    });
                }
            }
        }
    }

    if sweep_stmts.is_empty() && close_stmts.is_empty() && migration_finish_stmts.is_empty() {
        return Ok(quote! {});
    }

    let rent_fetch = if !migration_finish_stmts.is_empty() {
        quote! {
            let __shared_rent =
                <quasar_lang::sysvars::rent::Rent as quasar_lang::sysvars::Sysvar>::get()?;
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #[inline(always)]
        fn epilogue(&mut self) -> Result<(), ProgramError> {
            #rent_fetch
            #(#migration_finish_stmts)*
            #(#sweep_stmts)*
            #(#close_stmts)*
            Ok(())
        }
    })
}

pub(super) fn emit_realloc_steps(
    semantics: &[FieldSemantics],
) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    semantics
        .iter()
        .filter_map(|sem| sem.realloc.as_ref().map(|rc| (sem, rc)))
        .map(|(sem, rc)| emit_one_realloc(sem, rc))
        .collect()
}

fn emit_one_realloc(
    sem: &FieldSemantics,
    rc: &ReallocConstraint,
) -> syn::Result<proc_macro2::TokenStream> {
    let field = &sem.core.ident;
    let space = &rc.space_expr;
    let payer = sem
        .support
        .realloc_payer
        .clone()
        .ok_or_else(|| syn::Error::new(field.span(), "realloc requires a payer field"))?;

    Ok(quote! {
        {
            let __realloc_space = (#space) as usize;
            quasar_lang::accounts::realloc_account(
                #field, __realloc_space, #payer, Some(&__shared_rent)
            )?;
        }
    })
}

fn token_authority(sem: &FieldSemantics) -> Option<&syn::Ident> {
    sem.token
        .as_ref()
        .map(|tc| &tc.authority)
        .or_else(|| sem.ata.as_ref().map(|ac| &ac.authority))
}

fn token_mint(sem: &FieldSemantics) -> Option<&syn::Ident> {
    sem.token
        .as_ref()
        .map(|tc| &tc.mint)
        .or_else(|| sem.ata.as_ref().map(|ac| &ac.mint))
}

fn token_program(sem: &FieldSemantics) -> Option<&syn::Ident> {
    sem.support.token_program.as_ref()
}
