use napi_derive::napi;
use rspack_loader_swc::SwcDtsEmitOptions;

#[napi(object, object_to_js = false)]
pub struct RawSwcDtsEmitRspackPluginOptions {
  pub root_dir: Option<String>,
  pub include: Option<Vec<String>>,
}

impl From<RawSwcDtsEmitRspackPluginOptions> for SwcDtsEmitOptions {
  fn from(value: RawSwcDtsEmitRspackPluginOptions) -> Self {
    Self {
      root_dir: value.root_dir.unwrap(),
      include: value.include,
      out_dir: String::from("dist"),
    }
  }
}
