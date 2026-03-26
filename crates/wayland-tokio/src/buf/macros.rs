use bitfield::{bitfield, bitfield_fields};
use ecs_compositor_core::message_header;

pub struct Bitfield {
    pub lsb: u32,
    pub msb: u32,

    pub len: u32,
}

impl std::fmt::Debug for Bitfield {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { lsb, msb, len } = self;
        f.write_fmt(format_args!("{msb}..{lsb} = {len}"))
    }
}

macro_rules! gen_bitfield {
( struct $ty_name:ident
  $const:tt
  $(
      $vis:vis
      $type:ty,
      $name:ident,
      $set_name:ident,
      $with_name:ident

      = $size:expr;
  )*
) => {
    const _: () = {
        use $crate::buf::macros::Bitfield;

        #[allow(unused)]
        #[derive(Debug)]
        struct Fields {
            $(
                $name: $crate::buf::macros::Bitfield,
            )*
        }

        impl $ty_name {
            #[allow(dead_code, unused_assignments)]
            const FIELDS: Fields = {
                let mut offset: u32 = 0;

                gen_bitfield!{@unwrap_block $const}

                $(
                    let size = $size;
                    let len = usize::ilog2(size);

                    assert!(size == 1 << len);

                    let $name = Bitfield { msb: offset + len - 1, lsb: offset, len };

                    assert!(offset <= 64 && len <= <$type>::BITS);
                    offset += len;
                )*

                Fields { $( $name,)* }
            };

            bitfield_fields!{
                $(
                    $vis $type, $name, $set_name: Self::FIELDS.$name.msb as usize, Self::FIELDS.$name.lsb as usize;
                )*
            }

            $(
                $vis fn $with_name(&mut self, value: $type) -> &mut Self {
                    Self::$set_name(self, value);
                    self
                }
            )*
        }
    };
};
( @unwrap_block {$($block:tt)*}) => { $( $block )* };
}

bitfield! {
    struct Test(u64);
    impl Debug
}

gen_bitfield! {
    struct Test

    {
        let wayland_min_len = message_header::DATA_LEN as usize;
        let wayland_max_len = 1 << 16;

        let data_buf_len = wayland_max_len * 4;
        let ctrl_buf_len = 1024;
        let slot_buf_len = data_buf_len / wayland_min_len / 64;
    }

    pub u32, slot_chunk, set_slot_chunk, with_chunk = slot_buf_len;
    pub u8,  slot_index, set_slot_index, with_index = 64;

    pub u32, data,       set_data,       with_data = data_buf_len;
    pub u16, ctrl,       set_ctrl,       with_ctrl = ctrl_buf_len;
}

pub(crate) use gen_bitfield;
