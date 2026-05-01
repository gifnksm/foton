use std::io;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use snafu::{ResultExt as _, Snafu};

mod v1;

pub(in crate::db) use self::latest::types::*;

use v1 as latest;

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    schema_version: u32,
    payload: Box<RawValue>,
}

#[derive(Debug, Snafu)]
pub(crate) enum PersistError {
    #[snafu(display("failed to deserialize database envelope"))]
    DeserializeEnvelope { source: serde_json::Error },
    #[snafu(display("unknown database schema version: {}", schema_version))]
    UnknownSchemaVersion { schema_version: u32 },
    #[snafu(display("failed to deserialize database payload (schema version {schema_version})"))]
    DeserializePayload {
        schema_version: u32,
        source: serde_json::Error,
    },
    #[snafu(display("failed to serialize database payload (schema version {schema_version})"))]
    SerializePayload {
        schema_version: u32,
        source: serde_json::Error,
    },
    #[snafu(display("failed to serialize database envelope"))]
    SerializeEnvelope { source: serde_json::Error },
}

pub(in crate::db) fn from_reader<R>(reader: R) -> Result<PersistedPackageDb, PersistError>
where
    R: io::Read,
{
    let Envelope {
        schema_version,
        payload,
    }: Envelope = serde_json::from_reader(reader).context(DeserializeEnvelopeSnafu)?;

    let payload = match schema_version {
        v1::VERSION => v1::deserialize_payload(payload.get())?,
        schema_version => return Err(UnknownSchemaVersionSnafu { schema_version }.build()),
    };

    Ok(payload)
}

pub(in crate::db) fn to_writer<W>(
    writer: W,
    payload: &PersistedPackageDb,
) -> Result<(), PersistError>
where
    W: io::Write,
{
    let envelope = Envelope {
        schema_version: latest::VERSION,
        payload: latest::serialize_payload(payload)?,
    };

    serde_json::to_writer(writer, &envelope).context(SerializeEnvelopeSnafu)?;

    Ok(())
}
