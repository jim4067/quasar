//! Off-chain instruction building utilities.
//!
//! This module re-exports [`wincode`] for bincode-compatible serialization and
//! provides three wrapper types that encode Quasar's dynamic wire format:
//!
//! | Type | Wire format |
//! |------|-------------|
//! | [`DynBytes`] | `u32 LE` length prefix + raw bytes |
//! | [`DynVec<T>`] | `u32 LE` length prefix + each item serialized |
//! | [`TailBytes`] | raw bytes (no length prefix) |
//!
//! **This is the only module in `quasar-lang` that allocates** — it uses
//! `alloc::vec::Vec` for instruction data buffers since off-chain code runs
//! in a standard allocator environment.

extern crate alloc;

// Re-export instruction types used by generated client code.
pub use solana_instruction::{AccountMeta, Instruction};
// Re-export wincode for downstream derive macro codegen.
pub use wincode;
use {
    alloc::vec::Vec,
    core::mem::MaybeUninit,
    wincode::{
        config::ConfigCore,
        error::{ReadResult, WriteResult},
        io::{Reader, Writer},
        len::{SeqLen, UseIntLen},
        SchemaRead, SchemaWrite,
    },
};

/// Length encoding: little-endian `u32` prefix (Quasar wire format).
type U32Len = UseIntLen<u32>;

// ---------------------------------------------------------------------------
// DynBytes — u32-prefixed raw byte buffer
// ---------------------------------------------------------------------------

/// A dynamically-sized byte buffer prefixed with a `u32 LE` length.
///
/// Used in generated client code to serialize variable-length byte fields
/// (e.g. `String`, `Vec<u8>`) in instruction data.
pub struct DynBytes(pub Vec<u8>);

unsafe impl<C: ConfigCore> SchemaWrite<C> for DynBytes
where
    U32Len: SeqLen<C>,
{
    type Src = Self;

    fn size_of(src: &Self) -> WriteResult<usize> {
        Ok(U32Len::write_bytes_needed(src.0.len())? + src.0.len())
    }

    fn write(mut writer: impl Writer, src: &Self) -> WriteResult<()> {
        U32Len::write(writer.by_ref(), src.0.len())?;
        writer.write(&src.0)?;
        Ok(())
    }
}

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for DynBytes
where
    U32Len: SeqLen<C>,
{
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self>) -> ReadResult<()> {
        let len = U32Len::read(reader.by_ref())?;
        let bytes = reader.take_scoped(len)?;
        dst.write(DynBytes(bytes.to_vec()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DynVec<T> — u32-prefixed sequence of T
// ---------------------------------------------------------------------------

/// A dynamically-sized vector of `T` prefixed with a `u32 LE` element count.
///
/// Used in generated client code to serialize `Vec<T>` instruction arguments.
pub struct DynVec<T>(pub Vec<T>);

unsafe impl<T, C: ConfigCore> SchemaWrite<C> for DynVec<T>
where
    T: SchemaWrite<C, Src = T>,
    U32Len: SeqLen<C>,
{
    type Src = Self;

    fn size_of(src: &Self) -> WriteResult<usize> {
        let mut total = U32Len::write_bytes_needed(src.0.len())?;
        for item in &src.0 {
            total += T::size_of(item)?;
        }
        Ok(total)
    }

    fn write(mut writer: impl Writer, src: &Self) -> WriteResult<()> {
        U32Len::write(writer.by_ref(), src.0.len())?;
        for item in &src.0 {
            T::write(writer.by_ref(), item)?;
        }
        Ok(())
    }
}

unsafe impl<'de, T, C: ConfigCore> SchemaRead<'de, C> for DynVec<T>
where
    T: SchemaRead<'de, C, Dst = T>,
    U32Len: SeqLen<C>,
{
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self>) -> ReadResult<()> {
        let len = U32Len::read(reader.by_ref())?;
        let mut vec = Vec::with_capacity(len);
        for _ in 0..len {
            vec.push(T::get(reader.by_ref())?);
        }
        dst.write(DynVec(vec));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TailBytes — unprefixed trailing bytes
// ---------------------------------------------------------------------------

/// Raw trailing bytes with no length prefix.
///
/// On write, emits the raw bytes. On read, consumes all remaining bytes
/// from the reader. Useful for variable-length trailing data in instruction
/// payloads.
pub struct TailBytes(pub Vec<u8>);

unsafe impl<C: ConfigCore> SchemaWrite<C> for TailBytes {
    type Src = Self;

    fn size_of(src: &Self) -> WriteResult<usize> {
        Ok(src.0.len())
    }

    fn write(mut writer: impl Writer, src: &Self) -> WriteResult<()> {
        writer.write(&src.0)?;
        Ok(())
    }
}

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for TailBytes {
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self>) -> ReadResult<()> {
        // Consume all remaining bytes one at a time. This is only used
        // off-chain for instruction data deserialization, so the byte-at-a-time
        // approach is acceptable.
        let mut bytes = Vec::new();
        while let Ok(b) = reader.take_byte() {
            bytes.push(b);
        }
        dst.write(TailBytes(bytes));
        Ok(())
    }
}
