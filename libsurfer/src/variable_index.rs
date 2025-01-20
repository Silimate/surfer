use surfer_translation_types::VariableIndex;

#[local_impl::local_impl]
impl VariableIndexExt for VariableIndex {
    fn from_wellen_type(index: wellen::VarIndex) -> VariableIndex {
        VariableIndex {
            msb: index.msb(),
            lsb: index.lsb(),
        }
    }
}
