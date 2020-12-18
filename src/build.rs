use std::process::Command;
use std::path::Path;

#[macro_export]
macro_rules! make_path {
    ($path: expr) => {
        $path.to_str().unwrap()
    };
    ($path: expr, $($component: expr),*) => {
        $path
            $(.join($component))*
            .to_str().unwrap()
    };
}

pub fn build(command: &str, directory: Option<&Path>, args: &[&str], envs: &[(&str, &str)],
             fail_message: &str) -> bool {
    let mut to_run = Command::new(command);

    if let Some(directory) = directory {
        to_run.current_dir(directory);
    }

    let status = to_run
        .args(args)
        .envs(envs.iter().map(|&(k, v)| (k, v)))
        .status()
        .unwrap_or_else(|_| panic!("Invoking `{}` failed.", command));

    if !status.success() {
        println!("{}", fail_message);

        false
    } else {
        println!("Done!");
        true
    }
}

pub struct BuildParameters {
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
}

pub trait ImageBuilder {
    fn new(kernel_path: &Path, bootloader_dir: &Path,
           bootloader_build_dir: &Path) -> Self;

    fn bootloader_name() -> &'static str;
    fn image_name() -> &'static str;

    fn build_bootloader_dependencies(&mut self);
    fn bootloader_build_parameters(&mut self) -> BuildParameters;
    fn create_image(&mut self, image_path: &Path);
}
