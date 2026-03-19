//! Mailbox-specific runtime types and filesystem-backed mailbox helpers.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    BundleEnvelope, BundlePayload, MailboxMessageView, MessageKind, PublishedBundle,
    ReplyReference, RuntimeError,
    bundle::{MessageRef, Sequence},
    service::filesystem::{
        ACK_EXTENSION, ARTIFACTS_DIR_NAME, AckRecord, BODY_FILE_NAME, ENVELOPE_FILE_NAME,
        PROTOCOL_VERSION, RuntimeFileSystem, timestamp_now,
    },
};

/// Direction of a mailbox relative to the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MailboxDirection {
    /// Messages authored by the orchestrator and consumed by the worker.
    Inbox,
    /// Messages authored by the worker and consumed by the orchestrator.
    Outbox,
}

/// Acknowledgement reference for a consumed mailbox bundle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AckRef {
    /// Message being acknowledged.
    pub message: MessageRef,
    /// Acknowledged sequence number.
    pub sequence: Sequence,
}

impl RuntimeFileSystem {
    /// Publish a mailbox bundle and transfer any path-backed payloads.
    pub(crate) fn publish_bundle(
        &self, worktree_root: &Path, direction: MailboxDirection, kind: MessageKind,
        perspective: &crate::perspective::PerspectiveName, reply: ReplyReference,
        head_commit: Option<String>, payload: BundlePayload,
    ) -> Result<PublishedBundle, RuntimeError> {
        if payload.body_text.is_some() && payload.body_path.is_some() {
            return Err(RuntimeError::InvalidPayload(
                "body_text and body_path are mutually exclusive",
            ));
        }

        let worker_paths = crate::runtime::WorkerPaths::new(worktree_root.to_path_buf());
        let new_root = worker_paths.mailbox_new(direction);
        fs::create_dir_all(&new_root)?;

        let sequence = self.next_sequence(&new_root)?;
        let temp_dir = new_root.join(format!(".tmp-{}-{}", kind.slug(), sequence.0));
        let final_dir = new_root.join(format!("{:04}-{}", sequence.0, kind.slug()));

        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir)?;
        }

        fs::create_dir_all(temp_dir.join(ARTIFACTS_DIR_NAME))?;

        let envelope = BundleEnvelope {
            protocol: PROTOCOL_VERSION,
            perspective: perspective.clone(),
            kind,
            sequence,
            created_at: timestamp_now(),
            in_reply_to: reply.in_reply_to,
            head_commit,
        };
        Self::write_toml(&temp_dir.join(ENVELOPE_FILE_NAME), &envelope)?;
        self.write_bundle_body(
            &temp_dir.join(BODY_FILE_NAME),
            payload.body_text,
            payload.body_path,
        )?;
        self.move_artifacts(&temp_dir.join(ARTIFACTS_DIR_NAME), payload.artifacts)?;

        fs::rename(&temp_dir, &final_dir)?;

        Ok(PublishedBundle {
            message: MessageRef { perspective: perspective.clone(), kind, sequence },
            bundle_path: final_dir,
        })
    }

    /// Read mailbox bundles after an optional sequence threshold.
    pub(crate) fn list_mailbox_messages(
        &self, worktree_root: &Path, perspective: &crate::perspective::PerspectiveName,
        direction: MailboxDirection, after: Option<Sequence>,
    ) -> Result<Vec<MailboxMessageView>, RuntimeError> {
        let worker_paths = crate::runtime::WorkerPaths::new(worktree_root.to_path_buf());
        let new_root = worker_paths.mailbox_new(direction);
        let ack_root = worker_paths.mailbox_ack(direction);
        if !new_root.exists() {
            return Ok(Vec::new());
        }

        let mut messages = Vec::new();
        for entry in fs::read_dir(&new_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let bundle_path = entry.path();
            if bundle_path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".tmp-"))
            {
                continue;
            }

            let envelope: BundleEnvelope = Self::read_toml(&bundle_path.join(ENVELOPE_FILE_NAME))?;
            if let Some(after) = after {
                if envelope.sequence <= after {
                    continue;
                }
            }

            let acknowledged = ack_root.join(Self::ack_file_name(envelope.sequence)).exists();
            messages.push(MailboxMessageView {
                perspective: perspective.clone(),
                direction,
                kind: envelope.kind,
                sequence: envelope.sequence,
                created_at: envelope.created_at.clone(),
                acknowledged,
                head_commit: envelope.head_commit.clone(),
                summary: self.bundle_summary(&bundle_path, &envelope.kind),
                bundle_path,
            });
        }

        messages.sort_by_key(|message| message.sequence);
        Ok(messages)
    }

    /// Acknowledge one inbox or outbox bundle.
    pub(crate) fn acknowledge_message(
        &self, worktree_root: &Path, direction: MailboxDirection, sequence: Sequence,
    ) -> Result<AckRef, RuntimeError> {
        let worker_paths = crate::runtime::WorkerPaths::new(worktree_root.to_path_buf());
        let bundle_dir =
            self.find_bundle_by_sequence(&worker_paths.mailbox_new(direction), sequence)?;
        let envelope: BundleEnvelope = Self::read_toml(&bundle_dir.join(ENVELOPE_FILE_NAME))?;
        let ack_root = worker_paths.mailbox_ack(direction);
        fs::create_dir_all(&ack_root)?;
        let ack_path = ack_root.join(Self::ack_file_name(sequence));
        if ack_path.exists() {
            return Err(RuntimeError::AlreadyAcknowledged);
        }

        let ack = AckRecord { sequence, acknowledged_at: timestamp_now() };
        let mut file = OpenOptions::new().write(true).create_new(true).open(&ack_path)?;
        file.write_all(toml::to_string(&ack)?.as_bytes())?;

        Ok(AckRef {
            message: MessageRef {
                perspective: envelope.perspective,
                kind: envelope.kind,
                sequence,
            },
            sequence,
        })
    }

    fn next_sequence(&self, new_root: &Path) -> Result<Sequence, RuntimeError> {
        let mut max = 0_u64;
        if new_root.exists() {
            for entry in fs::read_dir(new_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy().to_string();
                let Some(prefix) = name.split('-').next() else {
                    continue;
                };
                if let Ok(parsed) = prefix.parse::<u64>() {
                    max = max.max(parsed);
                }
            }
        }
        Ok(Sequence(max + 1))
    }

    fn write_bundle_body(
        &self, target: &Path, body_text: Option<String>, body_path: Option<PathBuf>,
    ) -> Result<(), RuntimeError> {
        match (body_text, body_path) {
            | (Some(text), None) => fs::write(target, text)?,
            | (None, Some(path)) => move_file(&path, target)?,
            | (None, None) => fs::write(target, "")?,
            | (Some(_), Some(_)) => {
                return Err(RuntimeError::InvalidPayload(
                    "body_text and body_path are mutually exclusive",
                ));
            }
        }
        Ok(())
    }

    fn move_artifacts(
        &self, target_dir: &Path, artifacts: Vec<PathBuf>,
    ) -> Result<(), RuntimeError> {
        let mut seen = BTreeSet::new();
        for artifact in artifacts {
            let Some(name) = artifact.file_name() else {
                return Err(RuntimeError::InvalidPayload("artifact path must name a file"));
            };
            let name: OsString = name.to_owned();
            if !seen.insert(name.clone()) {
                return Err(RuntimeError::InvalidPayload("artifact names must be unique"));
            }
            move_file(&artifact, &target_dir.join(name))?;
        }
        Ok(())
    }

    fn bundle_summary(&self, bundle_path: &Path, kind: &MessageKind) -> String {
        let body_path = bundle_path.join(BODY_FILE_NAME);
        if let Ok(body) = fs::read_to_string(body_path) {
            if let Some(line) = body.lines().map(str::trim).find(|line| !line.is_empty()) {
                return line.to_owned();
            }
        }
        kind.slug().to_owned()
    }

    fn find_bundle_by_sequence(
        &self, new_root: &Path, sequence: Sequence,
    ) -> Result<PathBuf, RuntimeError> {
        if !new_root.exists() {
            return Err(RuntimeError::MessageNotFound);
        }

        for entry in fs::read_dir(new_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let bundle_path = entry.path();
            let envelope_path = bundle_path.join(ENVELOPE_FILE_NAME);
            if !envelope_path.exists() {
                continue;
            }
            let envelope: BundleEnvelope = Self::read_toml(&envelope_path)?;
            if envelope.sequence == sequence {
                return Ok(bundle_path);
            }
        }

        Err(RuntimeError::MessageNotFound)
    }

    /// The acknowledgement file name for one sequence number.
    fn ack_file_name(sequence: Sequence) -> String {
        format!("{:04}.{}", sequence.0, ACK_EXTENSION)
    }
}

/// Move a file into a runtime-managed location.
fn move_file(source: &Path, target: &Path) -> Result<(), RuntimeError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    match fs::rename(source, target) {
        | Ok(()) => Ok(()),
        | Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            fs::copy(source, target)?;
            fs::remove_file(source)?;
            Ok(())
        }
        | Err(error) => Err(RuntimeError::Io(error)),
    }
}
