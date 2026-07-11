use color_eyre::eyre::{self, eyre};
use serde_json::{Map, Value};

use crate::{scenario::ScenarioContext, util::env as env_util, util::fs as fs_util};

const INSTALLED_PKG_NAME: &str = "tom-thumb";
const ACTIVE_PKG_NAME: &str = "tom-thumb-monospace";

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    cx.install_fixture_config("foton-registry")?;
    setup_package(cx)?;
    create_broken_incomplete_packages(cx)?;

    // ensure that second repair attempt is no-op
    for _ in 0..2 {
        cx.exec_foton_with_args(["repair"])?.ensure_success()?;

        cx.ensure_package_not_managed(INSTALLED_PKG_NAME)?;
        cx.ensure_package_has_no_active_fonts(INSTALLED_PKG_NAME)?;
        cx.ensure_package_managed(ACTIVE_PKG_NAME)?
            .ensure_installation_state(|state| state.is_installed())?
            .ensure_activation_state(|state| state.is_inactive())?;
        cx.ensure_package_has_no_active_fonts(ACTIVE_PKG_NAME)?;
    }

    cx.uninstall_package(ACTIVE_PKG_NAME)?;

    Ok(())
}

fn setup_package(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    cx.ensure_package_not_managed(INSTALLED_PKG_NAME)?;
    cx.install_package_no_activation(INSTALLED_PKG_NAME)?;
    cx.ensure_package_not_managed(ACTIVE_PKG_NAME)?;
    cx.install_package(ACTIVE_PKG_NAME)?;

    Ok(())
}

const INSTALLATION_STATE_KEY: &str = "installation-state";
const ACTIVATION_STATE_KEY: &str = "activation-state";

fn create_broken_incomplete_packages(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    let db_json_path = env_util::foton_db_path()?;
    let mut db: Value = fs_util::read_json("db.json", &db_json_path)?;

    let Some(obj) = db.as_object_mut() else {
        return Err(eyre!("db.json is not a JSON object"));
    };
    update_package_state(obj, &mut |obj| {
        let is_active = obj.get(ACTIVATION_STATE_KEY).and_then(|v| v.as_str()) == Some("active");
        if is_active {
            obj.insert(
                ACTIVATION_STATE_KEY.into(),
                "incomplete-deactivation".into(),
            );
        } else {
            obj.insert(INSTALLATION_STATE_KEY.into(), "incomplete-uninstall".into());
        }
    });

    fs_util::write_json("db.json", &db_json_path, &db)?;

    cx.ensure_package_managed(INSTALLED_PKG_NAME)?
        .ensure_installation_state(|state| state.is_incomplete_uninstall())?
        .ensure_activation_state(|state| state.is_inactive())?;
    cx.ensure_package_managed(ACTIVE_PKG_NAME)?
        .ensure_installation_state(|state| state.is_installed())?
        .ensure_activation_state(|state| state.is_incomplete_deactivation())?;

    Ok(())
}

fn update_package_state<F>(obj: &mut Map<String, Value>, f: &mut F)
where
    F: FnMut(&mut Map<String, Value>),
{
    if obj.contains_key(INSTALLATION_STATE_KEY) && obj.contains_key(ACTIVATION_STATE_KEY) {
        f(obj);
        return;
    }
    for value in obj.values_mut() {
        match value {
            Value::Object(obj) => update_package_state(obj, f),
            Value::Array(arr) => {
                arr.iter_mut()
                    .filter_map(|item| item.as_object_mut())
                    .for_each(|obj| update_package_state(obj, f));
            }
            _ => {}
        }
    }
}
