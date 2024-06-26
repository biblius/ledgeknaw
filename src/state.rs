use crate::{
    document::{db::DocumentDb, process_root_directory, DocumentData, DocumentMeta},
    error::LedgeknawError,
};
use std::str::FromStr;
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::RwLock;
use tracing::{trace, warn};

#[derive(Debug, Clone)]
pub struct DocumentService {
    pub db: DocumentDb,

    /// The document title for the front end
    pub title: Arc<Option<String>>,

    /// The list of directories to initially include for the public page.
    /// Maps names to directory paths.
    pub directories: Arc<RwLock<HashMap<String, String>>>,
}

impl DocumentService {
    pub fn new(
        db: DocumentDb,
        title: Option<String>,
        directories: HashMap<String, String>,
    ) -> Self {
        Self {
            db,
            title: Arc::new(title),
            directories: Arc::new(RwLock::new(directories)),
        }
    }

    pub async fn sync(&self) -> Result<(), LedgeknawError> {
        let directories = self.directories.read().await;

        let paths = directories
            .values()
            .map(String::to_owned)
            .collect::<Vec<_>>();

        let full_paths = paths
            .iter()
            .map(|p| Path::new(p).canonicalize())
            .filter_map(Result::ok)
            .filter_map(|p| Some(p.to_str()?.to_owned()))
            .collect::<Vec<_>>();

        // Trim any root dirs that should not be loaded
        self.db.trim_roots(&full_paths).await?;

        // Trim any files and directories no longer on fs
        let file_paths = self.db.get_all_file_paths().await?;
        for path in file_paths {
            if let Err(e) = tokio::fs::metadata(&path).await {
                warn!("Error while reading file {path}, trimming");
                trace!("Error: {e}");
                self.db.remove_file_by_path(&path).await?;
            }
        }

        for (alias, path) in directories.iter() {
            process_root_directory(&self.db, path, alias).await?;
        }

        Ok(())
    }

    /// The `id` can either be the main identifier or a custom defined user id.
    pub async fn read_file(&self, id: String) -> Result<DocumentData, LedgeknawError> {
        let uuid = uuid::Uuid::from_str(&id);

        let Ok(uuid) = uuid else {
            let Some((id, path)) = self.db.get_doc_id_path_by_custom_id(&id).await? else {
                return Err(LedgeknawError::NotFound(id));
            };

            let document = DocumentData::read_from_disk(id, path)?;
            return Ok(document);
        };

        let doc_path = self.db.get_doc_path(uuid).await?;

        let Some(path) = doc_path else {
            return Err(LedgeknawError::NotFound(id));
        };

        let document = DocumentData::read_from_disk(uuid, path)?;
        Ok(document)
    }

    pub async fn get_file_meta(&self, id: uuid::Uuid) -> Result<DocumentMeta, LedgeknawError> {
        let doc_path = self.db.get_doc_path(id).await?;
        let Some(path) = doc_path else {
            return Err(LedgeknawError::NotFound(id.to_string()));
        };
        let meta = DocumentMeta::read_from_file(path)?;
        Ok(meta)
    }
}
