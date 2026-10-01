use std::time::{Duration, Instant};

use windows::{
    Win32::{
        Foundation::VARIANT_BOOL,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoSetProxyBlanket, CoUninitialize, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
            },
            UpdateAgent::{
                ISystemInformation, IUpdate2, IUpdateSession, SystemInformation, UpdateSession,
                orcSucceeded,
            },
            Variant::VARIANT,
            Wmi::{
                IWbemClassObject, IWbemLocator, WBEM_FLAG_FORWARD_ONLY,
                WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_S_FALSE, WBEM_S_TIMEDOUT, WbemLocator,
            },
        },
    },
    core::{BSTR, Interface, w},
};

use super::traits::{CollectBatch, CollectError};
use crate::domain::patch::{
    PatchRecord, PatchSource, PatchStatus, normalize_kb, parse_installed_date,
};

struct Apartment;
impl Apartment {
    fn enter() -> windows::core::Result<Self> {
        // This guard is created and destroyed on one spawn_blocking worker thread.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // Every successful CoInitializeEx, including S_FALSE, needs a matching release.
        unsafe {
            CoUninitialize();
        }
    }
}

pub(super) fn collect(
    name: &'static str,
    status: PatchStatus,
    timeout_secs: u64,
) -> Result<CollectBatch, CollectError> {
    let _apartment = Apartment::enter()
        .map_err(|e| CollectError::Backend(format!("{name} COM initialization: {e}")))?;
    if name == "wmi_installed" {
        wmi(timeout_secs)
    } else {
        wua(status).map_err(|e| CollectError::Backend(format!("{name} search: {e}")))
    }
}

fn property(
    object: &IWbemClassObject,
    name: windows::core::PCWSTR,
) -> windows::core::Result<Option<String>> {
    let mut value = VARIANT::default();
    // The object belongs to this apartment; VARIANT owns and clears the returned value.
    unsafe {
        object.Get(name, 0, &mut value, None, None)?;
    }
    if value.is_empty() {
        return Ok(None);
    }
    Ok(BSTR::try_from(&value)
        .ok()
        .map(|v| v.to_string())
        .filter(|s| !s.is_empty()))
}

fn wmi(timeout_secs: u64) -> Result<CollectBatch, CollectError> {
    let query = || -> windows::core::Result<CollectBatch> {
        // All COM objects remain on this initialized worker thread and drop before its apartment.
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let empty = BSTR::new();
            let services = locator.ConnectServer(
                &BSTR::from("ROOT\\CIMV2"),
                &empty,
                &empty,
                &empty,
                0,
                &empty,
                None,
            )?;
            // RPC_C_AUTHN_WINNT=10 and RPC_C_AUTHZ_NONE=0 are the documented COM constants.
            CoSetProxyBlanket(
                &services,
                10,
                0,
                None,
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )?;
            let rows = services.ExecQuery(&BSTR::from("WQL"), &BSTR::from("SELECT HotFixID, Description, Caption, InstalledOn, InstallDate FROM Win32_QuickFixEngineering"), WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, None)?;
            let started = Instant::now();
            let mut batch = CollectBatch::default();
            loop {
                if started.elapsed() >= Duration::from_secs(timeout_secs) {
                    return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                        0x800705B4_u32 as i32,
                    )));
                }
                let mut objects = [None];
                let mut returned = 0;
                let result = rows.Next(1000, &mut objects, &mut returned);
                result.ok()?;
                if returned == 0 {
                    if result.0 == WBEM_S_FALSE.0 {
                        break;
                    }
                    if result.0 == WBEM_S_TIMEDOUT.0 {
                        continue;
                    }
                    break;
                }
                if let Some(object) = objects[0].take() {
                    let id = property(&object, w!("HotFixID"))?.unwrap_or_default();
                    if id.trim().is_empty() {
                        return Err(windows::core::Error::new(
                            windows::core::HRESULT(0x80070057_u32 as i32),
                            "WMI returned a row without HotFixID",
                        ));
                    }
                    let kb = normalize_kb(&id).unwrap_or_else(|| format!("WMI:{}", id.trim()));
                    let mut record = PatchRecord::new(kb, PatchStatus::Installed, PatchSource::Wmi);
                    record.title = property(&object, w!("Caption"))?;
                    record.description = property(&object, w!("Description"))?;
                    record.category = Some("QuickFixEngineering".into());
                    if let Some(date) = property(&object, w!("InstalledOn"))? {
                        (record.installed_on, record.installed_date) = parse_installed_date(&date);
                        record.installed_on_raw = Some(date);
                    }
                    if let Some(date) = property(&object, w!("InstallDate"))?
                        && let Some(instant) = parse_installed_date(&date).0
                    {
                        record.installed_on = Some(instant);
                        record.installed_date = None;
                        record.installed_on_raw = Some(date);
                    }
                    batch.records.push(record);
                }
                if result.0 == WBEM_S_FALSE.0 {
                    break;
                }
            }
            Ok(batch)
        }
    };
    query().map_err(|e| CollectError::Backend(format!("wmi_installed query: {e}")))
}

fn wua(status: PatchStatus) -> windows::core::Result<CollectBatch> {
    // Generated interfaces own their COM references and stay inside the initialized apartment.
    unsafe {
        let session: IUpdateSession = CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER)?;
        session.SetClientApplicationID(&BSTR::from("PatchPulse"))?;
        let searcher = session.CreateUpdateSearcher()?;
        searcher.SetOnline(VARIANT_BOOL(0))?;
        let criteria = if status == PatchStatus::Installed {
            "IsInstalled=1"
        } else {
            "IsInstalled=0 and IsHidden=0"
        };
        let result = searcher.Search(&BSTR::from(criteria))?;
        let result_code = result.ResultCode()?;
        if result_code != orcSucceeded {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x80004005_u32 as i32),
                format!(
                    "WUA search did not fully succeed: result code {}",
                    result_code.0
                ),
            ));
        }
        let system: ISystemInformation =
            CoCreateInstance(&SystemInformation, None, CLSCTX_INPROC_SERVER)?;
        let mut batch = CollectBatch {
            reboot_required: Some(system.RebootRequired()?.0 != 0),
            ..CollectBatch::default()
        };
        let updates = result.Updates()?;
        for index in 0..updates.Count()? {
            let update = updates.get_Item(index)?;
            let identity = update.Identity()?;
            let guid = identity.UpdateID()?.to_string();
            let revision = identity.RevisionNumber()?;
            let mut record = PatchRecord::new(String::new(), status, PatchSource::Wua);
            record.update_id = Some(guid.clone());
            record.title = Some(update.Title()?.to_string()).filter(|s| !s.is_empty());
            record.description = Some(update.Description()?.to_string()).filter(|s| !s.is_empty());
            record.severity = Some(update.MsrcSeverity()?.to_string()).filter(|s| !s.is_empty());
            let update2: IUpdate2 = update.cast()?;
            record.reboot_required = update2.RebootRequired()?.0 != 0;
            let categories = update.Categories()?;
            let mut names = Vec::new();
            for i in 0..categories.Count()? {
                names.push(categories.get_Item(i)?.Name()?.to_string());
            }
            if !names.is_empty() {
                record.category = Some(names.join(", "));
            }
            let kbs = update.KBArticleIDs()?;
            if kbs.Count()? == 0 {
                record.kb_id = format!("WUA:{guid}:{revision}");
                batch.records.push(record);
            } else {
                for i in 0..kbs.Count()? {
                    let value = kbs.get_Item(i)?.to_string();
                    let kb = normalize_kb(&value).ok_or_else(|| {
                        windows::core::Error::from_hresult(windows::core::HRESULT(
                            0x80070057_u32 as i32,
                        ))
                    })?;
                    let mut item = record.clone();
                    item.kb_id = kb;
                    batch.records.push(item);
                }
            }
        }
        Ok(batch)
    }
}
