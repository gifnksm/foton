use std::io;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

mod v1;

pub(in crate::db) use self::latest::types::*;

use v1 as latest;

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    schema_version: u32,
    payload: Box<RawValue>,
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum PersistError {
    #[display("failed to deserialize database envelope")]
    DeserializeEnvelope {
        #[error(source)]
        source: serde_json::Error,
    },
    #[display("unknown database schema version: {}", schema_version)]
    UnknownSchemaVersion { schema_version: u32 },
    #[display("failed to deserialize database payload (schema version {schema_version})")]
    DeserializePayload {
        schema_version: u32,
        #[error(source)]
        source: serde_json::Error,
    },
    #[display("failed to serialize database payload (schema version {schema_version})")]
    SerializePayload {
        schema_version: u32,
        #[error(source)]
        source: serde_json::Error,
    },
    #[display("failed to serialize database envelope")]
    SerializeEnvelope {
        #[error(source)]
        source: serde_json::Error,
    },
}

pub(in crate::db) fn from_reader<R>(reader: R) -> Result<PersistedPackageDb, PersistError>
where
    R: io::Read,
{
    let Envelope {
        schema_version,
        payload,
    }: Envelope = serde_json::from_reader(reader)
        .map_err(|source| PersistError::DeserializeEnvelope { source })?;

    let payload = match schema_version {
        v1::VERSION => v1::deserialize_payload(payload.get())?,
        schema_version => return Err(PersistError::UnknownSchemaVersion { schema_version }),
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

    serde_json::to_writer(writer, &envelope)
        .map_err(|source| PersistError::SerializeEnvelope { source })?;

    Ok(())
}
