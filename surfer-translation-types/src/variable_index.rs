use std::fmt::{Display, Formatter};

/// An index that defines the extents of a variable.
/// For instance, in Verilog, this is declared using `[msb:lsb]`, e.g.:
/// ```verilog
/// reg [WIDTH-1:0] foo;
/// ```
///
/// A negative `lsb` usually indicates a fixed-point value where the
/// `[msb:0]` bits (including 0) belong to the integer part and the `[-1:lsb]` bits
/// belong to the fractional part of a number.
#[derive(Clone, Debug, Copy, Eq, PartialEq)]
pub struct VariableIndex {
    pub msb: i64,
    pub lsb: i64,
}

impl Display for VariableIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if self.msb == self.lsb {
            write!(f, "[{}]", self.lsb)
        } else {
            write!(f, "[{}:{}]", self.msb, self.lsb)
        }
    }
}
