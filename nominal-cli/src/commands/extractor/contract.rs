use crate::contract::{TimestampInput, version};
use nominal::core::{
    FileExtractionInput, FileExtractionParameter, ImageRegistration, RegisterableOutputFormat,
};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageContract {
    pub schema_version: u32,
    tag: String,
    output_format: OutputFormat,
    default_timestamp: TimestampInput,
    inputs: Vec<Input>,
    #[serde(default)]
    parameters: Vec<Parameter>,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum OutputFormat {
    Parquet,
    Csv,
    AvroStream,
    Manifest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    name: String,
    environment_variable: String,
    description: Option<String>,
    #[serde(default)]
    file_suffixes: Vec<String>,
    #[serde(default)]
    required: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameter {
    name: String,
    environment_variable: String,
    description: Option<String>,
    #[serde(default)]
    required: bool,
}
impl TryFrom<ImageContract> for ImageRegistration {
    type Error = anyhow::Error;
    fn try_from(v: ImageContract) -> anyhow::Result<Self> {
        version(v.schema_version)?;
        let format = match v.output_format {
            OutputFormat::Parquet => RegisterableOutputFormat::Parquet,
            OutputFormat::Csv => RegisterableOutputFormat::Csv,
            OutputFormat::AvroStream => RegisterableOutputFormat::AvroStream,
            OutputFormat::Manifest => RegisterableOutputFormat::Manifest,
        };
        let mut out = Self::new(v.tag, format, v.default_timestamp.try_into()?);
        for i in v.inputs {
            let mut input =
                FileExtractionInput::new(i.name, i.environment_variable).required(i.required);
            if let Some(d) = i.description {
                input = input.description(d)
            }
            for s in i.file_suffixes {
                input = input.suffix(s)
            }
            out = out.input(input);
        }
        for p in v.parameters {
            let mut param =
                FileExtractionParameter::new(p.name, p.environment_variable).required(p.required);
            if let Some(d) = p.description {
                param = param.description(d)
            }
            out = out.parameter(param);
        }
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extractor_image_contract_rejects_unknown_fields() {
        assert!(serde_json::from_str::<ImageContract>(r#"{"schema_version":1,"tag":"v1","output_format":"manifest","default_timestamp":{"column":"ts","kind":"epoch","unit":"nanoseconds"},"inputs":[],"activate":true}"#).is_err());
    }
}

#[cfg(test)]
mod conversion_tests {
    use super::*;
    #[test]
    fn extractor_image_contract_rejects_version_and_output_format() {
        let valid = r#"{"schema_version":1,"tag":"v1","output_format":"manifest","default_timestamp":{"column":"ts","kind":"epoch","unit":"nanoseconds"},"inputs":[{"name":"Recording","environment_variable":"RECORDING","file_suffixes":["flight"],"required":true}],"parameters":[{"name":"Parts","environment_variable":"PARTS"}]}"#;
        assert!(
            ImageRegistration::try_from(serde_json::from_str::<ImageContract>(valid).unwrap())
                .is_ok()
        );
        assert!(
            ImageRegistration::try_from(
                serde_json::from_str::<ImageContract>(
                    &valid.replace("\"schema_version\":1", "\"schema_version\":2")
                )
                .unwrap()
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ImageContract>(&valid.replace("manifest", "not-a-format"))
                .is_err()
        );
    }
}
