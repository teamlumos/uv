use std::process::Command;

use anyhow::Result;
use assert_fs::fixture::PathChild;
use assert_fs::fixture::{FileTouch, FileWriteStr};
use url::Url;

use common::uv_snapshot;

use crate::common::{get_bin, TestContext, EXCLUDE_NEWER, INSTA_FILTERS};

mod common;

/// Create a `pip install` command with options shared across scenarios.
fn command(context: &TestContext) -> Command {
    let mut command = Command::new(get_bin());
    command
        .arg("pip")
        .arg("install")
        .arg("--cache-dir")
        .arg(context.cache_dir.path())
        .arg("--exclude-newer")
        .arg(EXCLUDE_NEWER)
        .env("VIRTUAL_ENV", context.venv.as_os_str())
        .current_dir(&context.temp_dir);
    command
}

#[test]
fn empty() {
    let context = TestContext::new("3.12");

    uv_snapshot!(Command::new(get_bin())
        .arg("pip")
        .arg("list")
        .arg("--cache-dir")
        .arg(context.cache_dir.path())
        .env("VIRTUAL_ENV", context.venv.as_os_str())
        .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "###
    );
}

#[test]
fn single_no_editable() -> Result<()> {
    let context = TestContext::new("3.12");

    let requirements_txt = context.temp_dir.child("requirements.txt");
    requirements_txt.touch()?;
    requirements_txt.write_str("MarkupSafe==2.1.3")?;

    uv_snapshot!(command(&context)
        .arg("-r")
        .arg("requirements.txt")
        .arg("--strict"), @r###"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    Resolved 1 package in [TIME]
    Downloaded 1 package in [TIME]
    Installed 1 package in [TIME]
     + markupsafe==2.1.3
    "###
    );

    context.assert_command("import markupsafe").success();

    uv_snapshot!(Command::new(get_bin())
        .arg("pip")
        .arg("list")
        .arg("--cache-dir")
        .arg(context.cache_dir.path())
        .env("VIRTUAL_ENV", context.venv.as_os_str())
        .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
    Package    Version
    ---------- -------
    markupsafe 2.1.3  

    ----- stderr -----
    "###
    );

    Ok(())
}

#[test]
fn editable() -> Result<()> {
    let context = TestContext::new("3.12");

    let current_dir = std::env::current_dir()?;
    let workspace_dir = regex::escape(
        Url::from_directory_path(current_dir.join("..").join("..").canonicalize()?)
            .unwrap()
            .as_str(),
    );

    let filters = [(workspace_dir.as_str(), "file://[WORKSPACE_DIR]/")]
        .into_iter()
        .chain(INSTA_FILTERS.to_vec())
        .collect::<Vec<_>>();

    // Install the editable package.
    uv_snapshot!(filters, Command::new(get_bin())
        .arg("pip")
        .arg("install")
        .arg("-e")
        .arg("../../scripts/editable-installs/poetry_editable")
        .arg("--strict")
        .arg("--cache-dir")
        .arg(context.cache_dir.path())
        .arg("--exclude-newer")
        .arg(EXCLUDE_NEWER)
        .env("VIRTUAL_ENV", context.venv.as_os_str())
        .env("CARGO_TARGET_DIR", "../../../target/target_install_editable"), @r###"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    Built 1 editable in [TIME]
    Resolved 2 packages in [TIME]
    Downloaded 1 package in [TIME]
    Installed 2 packages in [TIME]
     + numpy==1.26.2
     + poetry-editable==0.1.0 (from file://[WORKSPACE_DIR]/scripts/editable-installs/poetry_editable)
    "###
    );

    // Account for difference length workspace dir
    let prefix = if cfg!(windows) { "file:///" } else { "file://" };

    // Origin of lengths used below:
    // - |Editable project location| = 25
    // - expected length = 57
    // - expected length - |Editable project location| = 32
    // - |`[WORKSPACE_DIR]/`| = 16
    // - |`file://`| = 7, |`file:///`| = 8 (windows)

    let workspace_len_difference = workspace_dir.as_str().len() + 32 - 16 - prefix.len();
    let find_divider = "-".repeat(25 + workspace_len_difference);
    let replace_divider = "-".repeat(57);

    let find_header = format!(
        "Editable project location{0}",
        " ".repeat(workspace_len_difference)
    );
    let replace_header = format!("Editable project location{0}", " ".repeat(32));

    let find_whitespace = " ".repeat(25 + workspace_len_difference);
    let replace_whitespace = " ".repeat(57);

    let search_workspace = workspace_dir.as_str().strip_prefix(prefix).unwrap();
    let replace_workspace = "[WORKSPACE_DIR]/";

    let filters = INSTA_FILTERS
        .iter()
        .copied()
        .chain(vec![
            (search_workspace, replace_workspace),
            (find_divider.as_str(), replace_divider.as_str()),
            (find_header.as_str(), replace_header.as_str()),
            (find_whitespace.as_str(), replace_whitespace.as_str()),
        ])
        .collect::<Vec<_>>();

    uv_snapshot!(filters, Command::new(get_bin())
    .arg("pip")
    .arg("list")
    .arg("--cache-dir")
    .arg(context.cache_dir.path())
    .env("VIRTUAL_ENV", context.venv.as_os_str())
    .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
    Package         Version Editable project location                                
    --------------- ------- ---------------------------------------------------------
    numpy           1.26.2                                                           
    poetry-editable 0.1.0   [WORKSPACE_DIR]/scripts/editable-installs/poetry_editable

    ----- stderr -----
    "###
    );

    Ok(())
}

#[test]
fn editable_only() -> Result<()> {
    let context = TestContext::new("3.12");

    let current_dir = std::env::current_dir()?;
    let workspace_dir = regex::escape(
        Url::from_directory_path(current_dir.join("..").join("..").canonicalize()?)
            .unwrap()
            .as_str(),
    );

    let filters = [(workspace_dir.as_str(), "file://[WORKSPACE_DIR]/")]
        .into_iter()
        .chain(INSTA_FILTERS.to_vec())
        .collect::<Vec<_>>();

    // Install the editable package.
    uv_snapshot!(filters, Command::new(get_bin())
        .arg("pip")
        .arg("install")
        .arg("-e")
        .arg("../../scripts/editable-installs/poetry_editable")
        .arg("--strict")
        .arg("--cache-dir")
        .arg(context.cache_dir.path())
        .arg("--exclude-newer")
        .arg(EXCLUDE_NEWER)
        .env("VIRTUAL_ENV", context.venv.as_os_str())
        .env("CARGO_TARGET_DIR", "../../../target/target_install_editable"), @r###"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    Built 1 editable in [TIME]
    Resolved 2 packages in [TIME]
    Downloaded 1 package in [TIME]
    Installed 2 packages in [TIME]
     + numpy==1.26.2
     + poetry-editable==0.1.0 (from file://[WORKSPACE_DIR]/scripts/editable-installs/poetry_editable)
    "###
    );

    // Account for difference length workspace dir
    let prefix = if cfg!(windows) { "file:///" } else { "file://" };

    let workspace_len_difference = workspace_dir.as_str().len() + 32 - 16 - prefix.len();
    let find_divider = "-".repeat(25 + workspace_len_difference);
    let replace_divider = "-".repeat(57);

    let find_header = format!(
        "Editable project location{0}",
        " ".repeat(workspace_len_difference)
    );
    let replace_header = format!("Editable project location{0}", " ".repeat(32));

    let find_whitespace = " ".repeat(25 + workspace_len_difference);
    let replace_whitespace = " ".repeat(57);

    let search_workspace = workspace_dir.as_str().strip_prefix(prefix).unwrap();
    let replace_workspace = "[WORKSPACE_DIR]/";

    let filters = INSTA_FILTERS
        .iter()
        .copied()
        .chain(vec![
            (search_workspace, replace_workspace),
            (find_divider.as_str(), replace_divider.as_str()),
            (find_header.as_str(), replace_header.as_str()),
            (find_whitespace.as_str(), replace_whitespace.as_str()),
        ])
        .collect::<Vec<_>>();

    uv_snapshot!(filters, Command::new(get_bin())
    .arg("pip")
    .arg("list")
    .arg("--editable")
    .arg("--cache-dir")
    .arg(context.cache_dir.path())
    .env("VIRTUAL_ENV", context.venv.as_os_str())
    .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
    Package         Version Editable project location                                
    --------------- ------- ---------------------------------------------------------
    poetry-editable 0.1.0   [WORKSPACE_DIR]/scripts/editable-installs/poetry_editable

    ----- stderr -----
    "###
    );

    uv_snapshot!(filters, Command::new(get_bin())
    .arg("pip")
    .arg("list")
    .arg("--exclude-editable")
    .arg("--cache-dir")
    .arg(context.cache_dir.path())
    .env("VIRTUAL_ENV", context.venv.as_os_str())
    .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
    Package Version
    ------- -------
    numpy   1.26.2 

    ----- stderr -----
    "###
    );

    uv_snapshot!(filters, Command::new(get_bin())
    .arg("pip")
    .arg("list")
    .arg("--editable")
    .arg("--exclude-editable")
    .arg("--cache-dir")
    .arg(context.cache_dir.path())
    .env("VIRTUAL_ENV", context.venv.as_os_str())
    .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "###
    );

    Ok(())
}

#[test]
fn exclude() -> Result<()> {
    let context = TestContext::new("3.12");

    let current_dir = std::env::current_dir()?;
    let workspace_dir = regex::escape(
        Url::from_directory_path(current_dir.join("..").join("..").canonicalize()?)
            .unwrap()
            .as_str(),
    );

    let filters = [(workspace_dir.as_str(), "file://[WORKSPACE_DIR]/")]
        .into_iter()
        .chain(INSTA_FILTERS.to_vec())
        .collect::<Vec<_>>();

    // Install the editable package.
    uv_snapshot!(filters, Command::new(get_bin())
        .arg("pip")
        .arg("install")
        .arg("-e")
        .arg("../../scripts/editable-installs/poetry_editable")
        .arg("--strict")
        .arg("--cache-dir")
        .arg(context.cache_dir.path())
        .arg("--exclude-newer")
        .arg(EXCLUDE_NEWER)
        .env("VIRTUAL_ENV", context.venv.as_os_str())
        .env("CARGO_TARGET_DIR", "../../../target/target_install_editable"), @r###"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    Built 1 editable in [TIME]
    Resolved 2 packages in [TIME]
    Downloaded 1 package in [TIME]
    Installed 2 packages in [TIME]
     + numpy==1.26.2
     + poetry-editable==0.1.0 (from file://[WORKSPACE_DIR]/scripts/editable-installs/poetry_editable)
    "###
    );

    // Account for difference length workspace dir
    let prefix = if cfg!(windows) { "file:///" } else { "file://" };

    let workspace_len_difference = workspace_dir.as_str().len() + 32 - 16 - prefix.len();
    let find_divider = "-".repeat(25 + workspace_len_difference);
    let replace_divider = "-".repeat(57);

    let find_header = format!(
        "Editable project location{0}",
        " ".repeat(workspace_len_difference)
    );
    let replace_header = format!("Editable project location{0}", " ".repeat(32));

    let find_whitespace = " ".repeat(25 + workspace_len_difference);
    let replace_whitespace = " ".repeat(57);

    let search_workspace = workspace_dir.as_str().strip_prefix(prefix).unwrap();
    let replace_workspace = "[WORKSPACE_DIR]/";

    let filters = INSTA_FILTERS
        .iter()
        .copied()
        .chain(vec![
            (search_workspace, replace_workspace),
            (find_divider.as_str(), replace_divider.as_str()),
            (find_header.as_str(), replace_header.as_str()),
            (find_whitespace.as_str(), replace_whitespace.as_str()),
        ])
        .collect::<Vec<_>>();

    uv_snapshot!(filters, Command::new(get_bin())
    .arg("pip")
    .arg("list")
    .arg("--exclude")
    .arg("numpy")
    .arg("--cache-dir")
    .arg(context.cache_dir.path())
    .env("VIRTUAL_ENV", context.venv.as_os_str())
    .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
    Package         Version Editable project location                                
    --------------- ------- ---------------------------------------------------------
    poetry-editable 0.1.0   [WORKSPACE_DIR]/scripts/editable-installs/poetry_editable

    ----- stderr -----
    "###
    );

    uv_snapshot!(filters, Command::new(get_bin())
    .arg("pip")
    .arg("list")
    .arg("--exclude")
    .arg("poetry-editable")
    .arg("--cache-dir")
    .arg(context.cache_dir.path())
    .env("VIRTUAL_ENV", context.venv.as_os_str())
    .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
    Package Version
    ------- -------
    numpy   1.26.2 

    ----- stderr -----
    "###
    );

    uv_snapshot!(filters, Command::new(get_bin())
    .arg("pip")
    .arg("list")
    .arg("--exclude")
    .arg("numpy")
    .arg("--exclude")
    .arg("poetry-editable")
    .arg("--cache-dir")
    .arg(context.cache_dir.path())
    .env("VIRTUAL_ENV", context.venv.as_os_str())
    .current_dir(&context.temp_dir), @r###"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "###
    );

    Ok(())
}
