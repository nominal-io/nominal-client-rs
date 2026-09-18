use super::{ContainerImageStatus, proto};
#[derive(Debug, Clone, Default)]
pub struct ExtractorQuery {
    pub(crate) include_archived: bool,
    pub(crate) file_extension: Option<String>,
}
impl ExtractorQuery {
    pub fn include_archived(mut self, value: bool) -> Self {
        self.include_archived = value;
        self
    }
    pub fn file_extension(mut self, value: impl Into<String>) -> Self {
        self.file_extension = Some(value.into());
        self
    }
}
#[derive(Debug, Clone, Default)]
pub struct ContainerImageQuery {
    extractor: Option<String>,
    tag: Option<String>,
    status: Option<ContainerImageStatus>,
}
impl ContainerImageQuery {
    pub fn extractor(mut self, value: impl Into<String>) -> Self {
        self.extractor = Some(value.into());
        self
    }
    pub fn tag(mut self, value: impl Into<String>) -> Self {
        self.tag = Some(value.into());
        self
    }
    pub fn status(mut self, value: ContainerImageStatus) -> Self {
        self.status = Some(value);
        self
    }
    pub(crate) fn into_proto(self) -> Option<proto::SearchFilter> {
        use proto::search_filter::Filter;
        let mut clauses = Vec::new();
        if let Some(extractor_rid) = self.extractor {
            clauses.push(proto::SearchFilter {
                filter: Some(Filter::Extractor(proto::ExtractorFilter { extractor_rid })),
            });
        }
        if let Some(tag) = self.tag {
            clauses.push(proto::SearchFilter {
                filter: Some(Filter::Tag(proto::TagFilter { tag })),
            });
        }
        if let Some(status) = self.status {
            clauses.push(proto::SearchFilter {
                filter: Some(Filter::Status(proto::StatusFilter {
                    status: status.into_proto(),
                })),
            });
        }
        if clauses.is_empty() {
            None
        } else {
            Some(proto::SearchFilter {
                filter: Some(Filter::And(proto::AndFilter { clauses })),
            })
        }
    }
}
