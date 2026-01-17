use std::num::NonZero;

pub(crate) const fn bit_mask_range(lower: u8, upper: u8) -> u64 {
    assert!(lower <= 63);
    assert!(upper <= 63);

    let (lower, upper) = (lower, upper + 1);

    match (lower, upper) {
        (l, u) if u <= l => 0,
        (64.., _) => 0,
        (l, 64..) => u64::MAX - ((1 << l) - 1),
        (l, u) => (1 << u) - (1 << l),
    }
}

pub(crate) const fn find_first_one(val: u64) -> Option<u8> {
    let Some(val) = NonZero::new(val) else {
        return None;
    };

    Some((u64::BITS - 1 - val.leading_zeros()) as u8)
}

pub struct WrapArgs<Lhs, Rhs, Lower, Upper, Diff> {
    pub lhs: Lhs,
    pub rhs: Rhs,

    pub lower: Lower,
    pub upper: Upper,

    pub diff: Diff,
}

/// fn wrapping_add(lhs: T, rhs: T, const WRAP_AT: T) -> T;
macro_rules! wrapping_add {
    (@init $($tail:tt)+ ) => {
        wrapping_add!(@add_lhs @ $($tail)+)
    };

    (@add_lhs @ $lhs:tt + $($tail:tt)+ ) => {
        wrapping_add!(@add_rhs $lhs @ $($tail)+)
    };

    (@add_rhs $lhs:tt @ $rhs:tt; $($tail:tt)*) => {
        wrapping_add!(@range $lhs $rhs @ $($tail)*)
    };

    (@range $lhs:tt $rhs:tt @ $lower:tt..$upper:tt ; $($tail:tt)*) => {
        wrapping_add!
        ( @cfg
          { do_diff: _
          , no_wrap: _
          , do_wrap: _
          }
          ($lhs,$rhs) ($lower,$upper)
          @ $($tail)*
        )
    };

    (@range $lhs:tt $rhs:tt @ $lower:tt..$upper:tt) => {
        wrapping_add!
        ( @cfg_defaults
          { do_diff: _
          , no_wrap: _
          , do_wrap: _
          }
          ($lhs,$rhs) ($lower,$upper)
          @
        )
    };


    ( @cfg
        { do_diff: _
        , no_wrap: $no_wrap:tt
        , do_wrap: $do_wrap:tt
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @ do_diff => $do_diff:expr,
        $($tail:tt)*
    ) => {
        wrapping_add!
        ( @cfg
          { do_diff: {$do_diff}
          , no_wrap:  $no_wrap
          , do_wrap:  $do_wrap
          }
          ($lhs,$rhs) ($lower,$upper)
          @ $($tail)*
        )
    };

    ( @cfg
        { do_diff: $do_diff:tt
        , no_wrap: _
        , do_wrap: $do_wrap:tt
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @ no_wrap => $no_wrap:expr,
        $($tail:tt)*
    ) => {
        wrapping_add!
        ( @cfg
          { do_diff:  $do_diff
          , no_wrap: {$no_wrap}
          , do_wrap:  $do_wrap
          }
          ($lhs,$rhs) ($lower,$upper)
          @ $($tail)*
        )
    };

    ( @cfg
        { do_diff: $do_diff:tt
        , no_wrap: $no_wrap:tt
        , do_wrap: _
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @ do_wrap => $do_wrap:expr,
        $($tail:tt)*
    ) => {
        wrapping_add!
        ( @cfg
          { do_diff:  $do_diff
          , no_wrap:  $no_wrap
          , do_wrap: {$do_wrap}
          }
          ($lhs,$rhs) ($lower,$upper)
          @ $($tail)*
        )
    };

    ( @cfg
        { do_diff: $do_diff:tt
        , no_wrap: $no_wrap:tt
        , do_wrap: $do_wrap:tt
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @
    ) => {
        wrapping_add!
        ( @cfg_defaults
          { do_diff:  $do_diff
          , no_wrap:  $no_wrap
          , do_wrap:  $do_wrap
          }
          ($lhs,$rhs) ($lower,$upper)
          @
        )
    };

    ( @cfg_defaults
        { do_diff: _
        , no_wrap: $no_wrap:tt
        , do_wrap: $do_wrap:tt
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @
    ) => {{
      use $crate::helpers::WrapArgs;
      let do_diff = |WrapArgs { lhs, upper, .. }| upper - lhs;
      wrapping_add!
      ( @cfg_defaults
        { do_diff: {do_diff}
        , no_wrap: $no_wrap
        , do_wrap: $do_wrap
        }
        ($lhs,$rhs) ($lower,$upper)
        @
      )
    }};

    ( @cfg_defaults
        { do_diff: $do_diff:tt
        , no_wrap: _
        , do_wrap: $do_wrap:tt
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @
    ) => {{
      use $crate::helpers::WrapArgs;
      let no_wrap = |WrapArgs { lhs, rhs, .. }| lhs + rhs;
      wrapping_add!
      ( @cfg_defaults
        { do_diff: $do_diff
        , no_wrap: {no_wrap}
        , do_wrap: $do_wrap
        }
        ($lhs,$rhs) ($lower,$upper)
        @
      )
    }};

    ( @cfg_defaults
        { do_diff: $do_diff:tt
        , no_wrap: $no_wrap:tt
        , do_wrap: _
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @
    ) => {{
      use $crate::helpers::WrapArgs;
      let do_wrap = |WrapArgs { rhs, lower, diff, .. }| lower + rhs - diff;
      wrapping_add!
      ( @cfg_defaults
        { do_diff: $do_diff
        , no_wrap: $no_wrap
        , do_wrap: {do_wrap}
        }
        ($lhs,$rhs) ($lower,$upper)
        @
      )
    }};

    ( @cfg_defaults
        { do_diff: $do_diff:tt
        , no_wrap: $no_wrap:tt
        , do_wrap: $do_wrap:tt
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @
    ) => {
        wrapping_add!
        ( @fin
          { do_diff:  $do_diff
          , no_wrap:  $no_wrap
          , do_wrap:  $do_wrap
          }
          ($lhs,$rhs) ($lower,$upper)
          @
        )
    };

    ( @fin
        { do_diff: {$do_diff:expr}
        , no_wrap: {$no_wrap:expr}
        , do_wrap: {$do_wrap:expr}
        }
        ($lhs:tt,$rhs:tt) ($lower:tt,$upper:tt)
        @
    ) => {{
        use $crate::helpers::WrapArgs;

        let lhs   = {$lhs};
        let rhs   = {$rhs};

        let lower = {$lower};
        let upper = {$upper};

        let diff  = ($do_diff)(WrapArgs { lhs, rhs, lower, upper, diff: () });
        if $rhs < diff { ($no_wrap)(WrapArgs { lhs, rhs, lower, upper, diff }) } else { ($do_wrap)(WrapArgs { lhs, rhs, lower, upper, diff }) }
    }};

    (@ $($tail:tt)*) => {
        compile_error!("error")
    };

    ($($tail:tt)*) => {
        wrapping_add!(@init $($tail)*)
    };
}

pub(crate) use wrapping_add;
