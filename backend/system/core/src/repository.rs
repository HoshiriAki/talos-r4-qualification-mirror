use crate::DataScope;
pub trait RepositoryScope {
    fn data_scope(&self) -> &DataScope;
}
impl RepositoryScope for DataScope {
    fn data_scope(&self) -> &DataScope {
        self
    }
}
