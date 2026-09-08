use super::*;
#[test]
fn extractor_update_preserves_unset_and_false() {
    let request = ExtractorUpdate::default()
        .archived(false)
        .into_request("e", "w");
    assert_eq!(request.name, None);
    assert_eq!(request.description, None);
    assert_eq!(request.is_archived, Some(false));
}
#[test]
fn image_unknown_values_and_absent_defaults_survive() {
    let image = ContainerImage::from_proto(
        proto::ContainerImage {
            status: 999,
            file_output_format: 888,
            ..Default::default()
        },
        "w".into(),
    )
    .unwrap();
    assert_eq!(image.status(), ContainerImageStatus::Unknown(999));
    assert_eq!(image.file_output_format(), FileOutputFormat::Unknown(888));
    assert!(image.default_timestamp().is_none());
}
#[test]
fn extraction_contract_roundtrips() {
    let input = FileExtractionInput::new("Source", "INPUT")
        .description("data")
        .suffix(".csv")
        .required(false);
    let encoded = input.clone().into_proto();
    assert_eq!(FileExtractionInput::from_proto(encoded).unwrap(), input);
    let parameter = FileExtractionParameter::new("Scale", "SCALE")
        .description("factor")
        .required(true);
    assert_eq!(
        FileExtractionParameter::from_proto(parameter.clone().into_proto()),
        parameter
    );
}
