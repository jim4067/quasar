// Migration codegen has moved to the runtime `Migration<From, To>::finish()`
// method. The derive now emits `self.field.finish(payer, &rent)?` in the
// epilogue (see lifecycle.rs). The old inline codegen that lived here is no
// longer needed since `#[account(migrate = X)]` is deprecated in favor of the
// `Migration<From, To>` field type.
//
// This module is kept as a placeholder for any future migration-related
// codegen utilities.
