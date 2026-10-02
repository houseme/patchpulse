//! Prepared RFC 4180 inventory rows with spreadsheet-safe text cells.
use crate::domain::patch::{PatchRecord, PatchSource, PatchStatus};
use bytes::Bytes;
use std::{borrow::Cow, ops::Range, sync::Arc};

pub(crate) const HEADER: &[u8] = b"kb_id,title,description,category,severity,installed_on,installed_date,installed_on_raw,status,reboot_required,source,sources,update_id\r\n";
type CsvSpans = Arc<Vec<Range<usize>>>;

fn cell(value: &str) -> Cow<'_, str> {
    if value.starts_with(['\t', '\r', '\n']) || value.trim_start().starts_with(['=', '+', '-', '@'])
    {
        Cow::Owned(format!("'{value}"))
    } else {
        Cow::Borrowed(value)
    }
}
fn source(value: PatchSource) -> &'static str {
    match value {
        PatchSource::Wmi => "wmi",
        PatchSource::Wua => "wua",
        PatchSource::PowerShell => "power_shell",
    }
}

pub(crate) fn encode(records: &[PatchRecord]) -> Result<(Bytes, CsvSpans), csv::Error> {
    if records.is_empty() {
        return Ok((Bytes::from_static(HEADER), Arc::default()));
    }
    let mut writer = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(HEADER.to_vec());
    let mut spans = Vec::with_capacity(records.len());
    for record in records {
        let start = writer.get_ref().len();
        let instant = record
            .installed_on
            .map(|value| value.to_string())
            .unwrap_or_default();
        let date = record
            .installed_date
            .map(|value| value.to_string())
            .unwrap_or_default();
        let sources = record
            .sources
            .iter()
            .map(|value| source(*value))
            .collect::<Vec<_>>()
            .join(";");
        let status = match record.status {
            PatchStatus::Installed => "installed",
            PatchStatus::Pending => "pending",
            PatchStatus::Failed => "failed",
            PatchStatus::Unknown => "unknown",
        };
        let fields = [
            cell(&record.kb_id),
            cell(record.title.as_deref().unwrap_or_default()),
            cell(record.description.as_deref().unwrap_or_default()),
            cell(record.category.as_deref().unwrap_or_default()),
            cell(record.severity.as_deref().unwrap_or_default()),
            cell(&instant),
            cell(&date),
            cell(record.installed_on_raw.as_deref().unwrap_or_default()),
            cell(status),
            cell(if record.reboot_required {
                "true"
            } else {
                "false"
            }),
            cell(source(record.source)),
            cell(&sources),
            cell(record.update_id.as_deref().unwrap_or_default()),
        ];
        writer.write_record(fields.iter().map(|value| value.as_bytes()))?;
        // Flush to the in-memory vector so each span includes exactly one complete CSV row.
        writer.flush()?;
        spans.push(start..writer.get_ref().len());
    }
    let bytes = writer
        .into_inner()
        .map_err(|error| csv::Error::from(error.into_error()))?;
    Ok((Bytes::from(bytes), Arc::new(spans)))
}
