use ecolor::Color32;
use pyo3::prelude::*;

use crate::ValueKind;

#[pymodule]
#[pyo3(name = "surfer")]
pub fn surfer_pyo3_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PythonBasicTranslator>()?;
    m.add_class::<PythonValueKind>()?;
    Ok(())
}

#[pyclass(name = "BasicTranslator", subclass)]
struct PythonBasicTranslator {}
// NOTE: No implementation for the PythonBasicTranslator here. Will be done later.

#[derive(Clone)]
#[pyclass(name = "ValueKind")]
pub enum PythonValueKind {
    Normal {},
    Undef {},
    HighImp {},
    Custom { color: [u8; 4] },
    Warn {},
    DontCare {},
    Weak {},
}

impl From<PythonValueKind> for ValueKind {
    fn from(value: PythonValueKind) -> Self {
        match value {
            PythonValueKind::Normal {} => ValueKind::Normal,
            PythonValueKind::Undef {} => ValueKind::Undef,
            PythonValueKind::HighImp {} => ValueKind::HighImp,
            PythonValueKind::Custom {
                color: [r, g, b, a],
            } => ValueKind::Custom(Color32::from_rgba_unmultiplied(r, g, b, a)),
            PythonValueKind::Warn {} => ValueKind::Undef,
            PythonValueKind::DontCare {} => ValueKind::Undef,
            PythonValueKind::Weak {} => ValueKind::Undef,
        }
    }
}
