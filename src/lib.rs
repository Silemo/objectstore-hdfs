use std::collections::{BTreeSet, VecDeque};
use std::fmt::{Display, Formatter};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{stream::BoxStream, StreamExt};

use hdfs::hdfs::{get_hdfs_by_full_path, FileStatus, HdfsErr, HdfsFile, HdfsFs};
use hdfs::walkdir::HdfsWalkDir;
use object_store::{
    path::{self, Path}, Error, GetOptions, GetResult, GetResultPayload, 
    ListResult, ObjectMeta, ObjectStore, PutOptions, PutResult,
    PutMode, Result, Attributes, PutMultipartOpts, GetRange,
    // TODO: comment next line for Version 0.9
    MultipartUpload, PutPayload,
    //util::{self, maybe_spawn_blocking}, 
};

// TODO: Version 0.9
// use tokio::io::AsyncWrite;

// HopsFS server name and hdfs local directory
const HOPS_FS_FULL: &str = "hdfs://rpc.namenode.service.consul:8020/user/hdfs/tests/";
const HOPS_FS_PREFIX: &str = "hdfs://rpc.namenode.service.consul:8020/";
const HOPS_FS_LOC_PATH: &str = "/user/hdfs/tests/";
const HOPS_FS_TESTS: &str = "/tests/";
const HOPS_FS_TESTS_NO_SLASH: &str = "tests";

#[derive(Debug)]
pub struct HadoopFileSystem {
    hdfs: Arc<HdfsFs>,
}

impl Display for HadoopFileSystem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "HadoopFileSystem({})", self.get_path_root())
    }
}

impl Default for HadoopFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl HadoopFileSystem {
    /// Create new HadoopFileSystem by getting HdfsFs from default path
    pub fn new() -> Self {
        Self {
            hdfs: get_hdfs_by_full_path(HOPS_FS_FULL).expect("Fail to get default HdfsFs"),
        }
    }

    /// Create new HadoopFileSystem by getting HdfsFs from the full path, 
    /// e.g. full_path == hdfs://localhost:8020/xxx/xxx
    pub fn new_from_full_path(full_path: &str) -> Option<Self> {
        get_hdfs_by_full_path(full_path)
            .map(|hdfs| Some(Self { hdfs }))
            .unwrap_or(None)
    }

    pub fn get_path_root(&self) -> String {
        let root = self.hdfs.url().to_owned();
        print!("get_path_root - ROOT : {} \n", root);
        format!("{}{}", self.hdfs.url().to_owned(), HOPS_FS_LOC_PATH)
    }

    pub fn get_path(&self, full_path: &str) -> Path {
        get_path(full_path, &self.get_path_root())
    }

    fn read_range(range: &Range<usize>, file: &HdfsFile) -> Result<Bytes> {
        // Set lenght to read
        let to_read = range.end - range.start;
        
        // Create buffer to safe what was read
        let mut buf = vec![0; to_read];
        
        // Read specifying the range
        let read = file
            .read_with_pos(range.start as i64, buf.as_mut_slice())
            .map_err(match_error)?;

        // Verify read
        assert_eq!(
            to_read as i32,
            read,
            "Read path {} from {} with expected size {} and actual size {}",
            file.path(),
            range.start,
            to_read,
            read
        );

        Ok(buf.into())
    }
}


#[async_trait]
impl ObjectStore for HadoopFileSystem {
    /// Save the provided `payload` to `location` with the given options (TODO: here payload: PutPayload)
    async fn put_opts(&self, location: &Path, payload: PutPayload, opts: PutOptions) -> Result<PutResult> {
        if matches!(opts.mode, PutMode::Update(_)) {
            return Err(Error::NotImplemented);
        }

        if !opts.attributes.is_empty() {
            return Err(Error::NotImplemented);
        }

        let hdfs = self.hdfs.clone();
        // The following variable will shadow location: &Path
        let location = String::from(location.clone());
        print!("put_opts - LOCATION: {} \n", location);
        maybe_spawn_blocking(move || {
            
            let result = payload.iter().try_for_each(|bytes| {
                // Note that the variable file either becomes a HdfsFile f, or an error
                let (file, err) = match opts.mode {
                    PutMode::Overwrite => {
                        print!("put_opts - Create with overwrite {} \n", location);
                        match hdfs.create_with_overwrite(&location, true) {
                            Ok(f) => {
                                print!("put_opts - OK -> Create with overwrite \n");
                                (Some(f), None)
                            },
                            Err(e) => (None, Some(e)),
                        }
                    }
                    PutMode::Create => {
                        print!("put_opts - CREATE \n");
                        match hdfs.create(&location) {
                            Ok(f) => {
                                print!("put_opts - OK -> Create \n");
                                (Some(f), None)
                            },
                            Err(e) => (None, Some(e)),
                        }
                    }
                    PutMode::Update(_) => unreachable!(),
                };

                if err.is_some() {
                    return Err(match_error(err.expect("Hdfs Error Reason")));
                }

                let file = file.unwrap();
                print!("put_opts - WRITING on file \n");
                file.write(bytes.as_ref()).map_err(match_error)?;
                print!("put_opts - COMPLETED writing on file \n");
                let result = file.close().map_err(match_error);
                if result.is_err() {
                    return Err(match_error(HdfsErr::Generic("Error closing the HDFS file".to_string())))
                }
                Ok(())
            });   

            print!("put_opts: Out of maybe_spawn_blocking function \n");
            match result {
                Ok(()) => {
                    return Ok(PutResult {
                        e_tag: None,
                        version: None,
                    });
                },
                Err(err) => return Err(err),
            }
        })
        .await
    }

    /// TODO: Version 0.9
    /// Perform a multipart upload
    ///
    /// Client should prefer [`ObjectStore::put`] for small payloads, as streaming uploads
    /// typically require multiple separate requests. See [`MultipartUpload`] for more information
    // TODO here -> Result<Box<dyn MultipartUpload>>
    //async fn put_multipart(&self, location: &Path) -> Result<Box<dyn MultipartUpload>> {  
        //todo!()
    //}

    /// TODO: Version 0.9
    /// Cleanup an aborted upload.
    ///
    /// See documentation for individual stores for exact behavior, as capabilities
    /// vary by object store.
    //async fn abort_multipart(&self, location: &Path, multipart_id: &MultipartId) -> Result<()> {
    //    todo!()
    //}
    
    // VERSION 0.10
    /// Perform a multipart upload with options
    ///
    /// Client should prefer [`ObjectStore::put`] for small payloads, as streaming uploads
    /// typically require multiple separate requests. See [`MultipartUpload`] for more information
    async fn put_multipart_opts(&self, _location: &Path, _opts: PutMultipartOpts) -> Result<Box<dyn MultipartUpload>> {
        
        todo!()
    }

    /// Perform a get request with options
    async fn get_opts(&self, location: &Path, options: GetOptions) -> Result<GetResult> {
        if options.if_match.is_some() || options.if_none_match.is_some() {
            return Err(Error::Generic {
                store: "HadoopFileSystem",
                source: Box::new(HdfsErr::Generic("ETags not supported".to_string())),
            });
        }

        let hdfs = self.hdfs.clone();
        let hdfs_root = self.get_path_root();
        // The following variable will shadow location: &Path
        let location = String::from(location.clone());

        let (blob, object_metadata, range) = maybe_spawn_blocking(move || {
            let file = hdfs.open(&location).map_err(match_error)?;
            let file_status = file.get_file_status().map_err(match_error)?;

            // Check Modified
            if options.if_unmodified_since.is_some() || options.if_modified_since.is_some() {
                check_modified(&options, &location, last_modified(&file_status))?;
            }

            // Set range
            let range = match options.range.unwrap() {
                GetRange::Bounded(range) => range,
                _ => Range {
                    start: 0,
                    end: file_status.len(),
                }
            };

            // Read Buffer
            let buf = Self::read_range(&range, &file)?;

            // Close file
            file.close().map_err(match_error)?;

            // Convert Metadata
            let object_metadata = convert_metadata(file_status, &hdfs_root);

            Ok((buf, object_metadata, range))
        })
        .await?;

        Ok(GetResult {
            payload: GetResultPayload::Stream(
                futures::stream::once(async move { Ok(blob) }).boxed(),
            ),
            meta: object_metadata,
            range,
            attributes: Attributes::default(),
        })
    }

    /// Return the bytes that are stored at the specified location
    /// in the given byte range.
    ///
    /// See [`GetRange::Bounded`] for more details on how `range` gets interpreted
    async fn get_range(&self, location: &Path, range: Range<usize>) -> Result<Bytes> {
        let hdfs = self.hdfs.clone();
        // The following variable will shadow location: &Path
        let location = String::from(location.clone());

        maybe_spawn_blocking(move || {
            let file = hdfs.open(&location).map_err(match_error)?;
            let buf = Self::read_range(&range, &file)?;
            file.close().map_err(match_error)?;

            Ok(buf)
        })
        .await
    }

    /// Return the metadata for the specified location
    async fn head(&self, location: &Path) -> Result<ObjectMeta> {
        let hdfs = self.hdfs.clone();
        let hdfs_root = self.get_path_root();
        // The following variable will shadow location: &Path
        let location = String::from(location.clone());

        maybe_spawn_blocking(move || {
            let file_status = hdfs.get_file_status(&location).map_err(match_error)?;
            Ok(convert_metadata(file_status, &hdfs_root))
        })
        .await
    }

    /// Delete the object at the specified location.
    async fn delete(&self, location: &Path) -> Result<()> {
        let hdfs = self.hdfs.clone();
        // The following variable will shadow location: &Path
        let location = format!("{}{}", HOPS_FS_TESTS, String::from(location.clone()));

        maybe_spawn_blocking(move || {
            hdfs.delete(&location, false).map_err(match_error)?;

            Ok(())
        })
        .await
    }

    /// List all of the leaf files under the prefix path.
    /// It will recursively search leaf files whose depth is larger than 1
    fn list(&self, prefix: Option<&Path>) -> BoxStream<'_, Result<ObjectMeta>> {
        let default_path = Path::from(self.get_path_root());
        let prefix = prefix.unwrap_or(&default_path);
        let prefix = {
            if !String::from(prefix.as_ref()).starts_with(HOPS_FS_TESTS_NO_SLASH) {
                Path::from(format!("{}{}", HOPS_FS_TESTS,  String::from(prefix.as_ref())))
            } else {
                prefix.clone()
            }
        };
        print!("list - PREFIX: {} \n", String::from(prefix.clone()));
        let hdfs = self.hdfs.clone();
        let hdfs_root = self.get_path_root();
        print!("list - hdfs_root: {} \n", hdfs_root);
        let walkdir =
            HdfsWalkDir::new_with_hdfs(String::from(prefix.clone()), hdfs)
                .min_depth(1);

        let s =
            walkdir.into_iter().flat_map(move |result_dir_entry| {
                match convert_walkdir_result(result_dir_entry) {
                    Err(e) => Some(Err(e)),
                    Ok(None) => None,
                    Ok(entry @ Some(_)) => {
                        print!("list - ENTRY: {} \n", entry.as_ref().unwrap().name());
                        entry
                        .filter(|dir_entry| dir_entry.is_file())
                        .map(|entry| Ok(convert_metadata(entry, &hdfs_root)))
                    },
                }
            });

        // If no tokio context, return iterator directly as no
        // need to perform chunked spawn_blocking reads
        if tokio::runtime::Handle::try_current().is_err() {
            return futures::stream::iter(s).boxed();
        }

        // Otherwise list in batches of CHUNK_SIZE
        const CHUNK_SIZE: usize = 1024;

        let buffer = VecDeque::with_capacity(CHUNK_SIZE);
        let stream = futures::stream::try_unfold((s, buffer), |(mut s, mut buffer)| async move {
            if buffer.is_empty() {
                (s, buffer) = tokio::task::spawn_blocking(move || {
                    for _ in 0..CHUNK_SIZE {
                        match s.next() {
                            Some(r) => buffer.push_back(r),
                            None => break,
                        }
                    }
                    (s, buffer)
                })
                .await?;
            }

            match buffer.pop_front() {
                Some(Err(e)) => Err(e),
                Some(Ok(meta)) => Ok(Some((meta, (s, buffer)))),
                None => Ok(None),
            }
        });

        stream.boxed()
    }

    /// List files and directories directly under the prefix path.
    /// It will not recursively search leaf files whose depth is larger than 1
    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> Result<ListResult> {
        let default_path = Path::from(self.get_path_root());
        let prefix = prefix.unwrap_or(&default_path);
        print!("list_with_delimiter - PREFIX: {} \n", prefix);
        let hdfs = self.hdfs.clone();
        let hdfs_root = self.get_path_root();
        print!("list_with_delimiter - hdfs_root: {} \n", hdfs_root);
        let walkdir =
            HdfsWalkDir::new_with_hdfs(String::from(prefix.clone()), hdfs)
                .min_depth(1)
                .max_depth(1);

        let prefix = prefix.clone();
        maybe_spawn_blocking(move || {
            let mut common_prefixes = BTreeSet::new();
            print!("list_with_delimiter - common_prefixes length: {} \n", common_prefixes.len());
            let mut objects = Vec::new();

            for entry_res in walkdir.into_iter().map(convert_walkdir_result) {
                if let Some(entry) = entry_res? {
                    print!("list_with_delimiter - ENTRY: {} \n", &entry.name());
                    let is_directory = entry.is_directory();
                    print!("list_with_delimiter - is_directory: {} \n", is_directory);
                    let entry_location = get_path(entry.name(), &hdfs_root);

                    let mut parts = match entry_location.prefix_match(&prefix) {
                        Some(parts) => {
                            print!("list_with_delimiter - PARTS Some \n");
                            parts
                        },
                        None => {
                            print!("list_with_delimiter - PARTS None \n");
                            continue
                        },
                    };

                    let common_prefix = match parts.next() {
                        Some(p) => {
                            print!("list_with_delimiter - common_prefix Some \n");
                            p
                        },
                        None => {
                            print!("list_with_delimiter - common_prefix None \n");
                            continue
                        },
                    };

                    drop(parts);

                    if is_directory {
                        print!("list_with_delimiter - is_directory condition \n");
                        common_prefixes.insert(prefix.child(common_prefix));
                    } else {
                        print!("list_with_delimiter - push object \n");
                        objects.push(convert_metadata(entry, &hdfs_root));
                    }
                }
                else {
                    print!("list_with_delimiter - None entry value here! \n");
                }
            }

            Ok(ListResult {
                common_prefixes: common_prefixes.into_iter().collect(),
                objects,
            })
        })
        .await
    }

    /// Copy an object from one path to another.
    /// If there exists an object at the destination, it will be overwritten.
    async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        let hdfs = self.hdfs.clone();
        // The following two variables will shadow from: &Path and to: &Path
        let from = String::from(from.clone());
        let to = String::from(to.clone());

        maybe_spawn_blocking(move || {
            // We need to make sure the source exist
            if !hdfs.exist(&from) {
                return Err(Error::NotFound {
                    path: from.clone(),
                    source: Box::new(HdfsErr::FileNotFound(from)),
                });
            }
            // Delete destination if exists
            if hdfs.exist(&to) {
                hdfs.delete(&to, false).map_err(match_error)?;
            }

            hdfs::util::HdfsUtil::copy(hdfs.as_ref(), &from, hdfs.as_ref(), &to)
                .map_err(match_error)?;

            Ok(())
        })
        .await
    }

    /// Move an object from one path to another in the same object store (HDFS)
    async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        let hdfs = self.hdfs.clone();
        // The following two variables will shadow the from and to &Path
        let from = String::from(from.clone());
        let to = String::from(to.clone());

        maybe_spawn_blocking(move || {
            hdfs.rename(&from, &to).map_err(match_error)?;

            Ok(())
        })
        .await
    }

    /// Copy an object from one path to another, only if destination is empty.
    ///
    /// Will return an error if the destination already has an object. 
    /// This is done performing an atomic operation
    async fn copy_if_not_exists(&self, from: &Path, to: &Path) -> Result<()> {
        let hdfs = self.hdfs.clone();
        // The following two variables will shadow the from and to &Path
        let from = String::from(from.clone());
        let to = String::from(to.clone());

        maybe_spawn_blocking(move || {
            if hdfs.exist(&to) {
                return Err(Error::AlreadyExists {
                    path: from,
                    source: Box::new(HdfsErr::FileAlreadyExists(to)),
                });
            }

            hdfs::util::HdfsUtil::copy(hdfs.as_ref(), &from, hdfs.as_ref(), &to)
                .map_err(match_error)?;

            Ok(())
        })
        .await
    }
}

/// Matches HdfsErr to its corresponding ObjectStoreError
fn match_error(err: HdfsErr) -> Error {
    match err {
        HdfsErr::FileNotFound(path) => Error::NotFound {
            path: path.clone(),
            source: Box::new(HdfsErr::FileNotFound(path)),
        },
        HdfsErr::FileAlreadyExists(path) => Error::AlreadyExists {
            path: path.clone(),
            source: Box::new(HdfsErr::FileAlreadyExists(path)),
        },
        HdfsErr::InvalidUrl(path) => Error::InvalidPath {
            source: path::Error::InvalidPath {
                path: PathBuf::from(path),
            },
        },
        HdfsErr::CannotConnectToNameNode(namenode_uri) => Error::Generic {
            store: "HadoopFileSystem",
            source: Box::new(HdfsErr::CannotConnectToNameNode(namenode_uri)),
        },
        HdfsErr::Generic(err_str) => Error::Generic {
            store: "HadoopFileSystem",
            source: Box::new(HdfsErr::Generic(err_str)),
        },
    }
}

/// Create Path without prefix
pub fn get_path(full_path: &str, prefix: &str) -> Path {
    print!("get_path - PREFIX: {} \n", prefix);
    let partial_path = full_path.strip_prefix(prefix).unwrap();
    print!("get_path - Partial Path: {} \n", &partial_path);
    Path::from(partial_path)
}

/// Convert HDFS file status to ObjectMeta
pub fn convert_metadata(file: FileStatus, prefix: &str) -> ObjectMeta {
    ObjectMeta {
        location: get_path(file.name(), prefix),
        last_modified: last_modified(&file),
        size: file.len(),
        e_tag: None,
        version: None,
    }
}

/// Gets the last_modified date time on a file
fn last_modified(file: &FileStatus) -> DateTime<Utc> {
    DateTime::from_timestamp(file.last_modified(), 0).unwrap()
}

/// Checks if the file at the specified location was modified since a specific date
fn check_modified(
    get_options: &GetOptions,
    location: &str,
    last_modified: DateTime<Utc>,
) -> Result<()> {
    if let Some(date) = get_options.if_modified_since {
        if last_modified <= date {
            return Err(Error::NotModified {
                path: location.to_string(),
                source: format!("{} >= {}", date, last_modified).into(),
            });
        }
    }

    if let Some(date) = get_options.if_unmodified_since {
        if last_modified > date {
            return Err(Error::Precondition {
                path: location.to_string(),
                source: format!("{} < {}", date, last_modified).into(),
            });
        }
    }
    Ok(())
}

/// Convert walkdir results and converts not-found errors into `None`.
fn convert_walkdir_result(
    res: std::result::Result<FileStatus, HdfsErr>,
) -> Result<Option<FileStatus>> {
    match res {
        Ok(entry) => Ok(Some(entry)),
        Err(walkdir_err) => match walkdir_err {
            HdfsErr::FileNotFound(_) => Ok(None),
            _ => Err(match_error(HdfsErr::Generic(
                "Fail to walk hdfs directory".to_owned(),
            ))),
        },
    }
}

/// Takes a function and spawns it to a tokio blocking pool if available
pub async fn maybe_spawn_blocking<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    //#[cfg(feature = "try_spawn_blocking")]
    match tokio::runtime::Handle::try_current() {
        Ok(runtime) => runtime.spawn_blocking(f).await?,
        Err(_) => f(),
    }

    //#[cfg(not(feature = "try_spawn_blocking"))]
    //f()
}

#[cfg(test)]
mod tests_util {

    use object_store::{
        Error, Result, DynObjectStore, path::Path, GetOptions,
        GetRange, PutPayload, ObjectStore, Attributes, Attribute,
        PutMode, UpdateVersion, WriteMultipart, multipart::MultipartStore,
        ObjectMeta,
    };
    use futures::stream::{FuturesUnordered, BoxStream};
    use futures::StreamExt;
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};
    use futures::TryStreamExt;
    use bytes::Bytes;

    pub async fn flatten_list_stream(
        storage: &DynObjectStore,
        prefix: Option<&Path>,
    ) -> Result<Vec<Path>> {
        storage
            .list(prefix)
            .map_ok(|meta| meta.location)
            .try_collect::<Vec<Path>>()
            .await
    }

    pub async fn put_get_delete_list(storage: &DynObjectStore) {
        delete_fixtures(storage).await;

        let content_list = flatten_list_stream(storage, None).await.unwrap();
        assert!(
            content_list.is_empty(),
            "Expected list to be empty; found: {content_list:?}"
        );

        let location = Path::from("test_dir/test_file.json");

        let data = Bytes::from("arbitrary data");
        print!("put_get_delete - Before PUT function \n");
        storage.put(&location, data.clone().into()).await.unwrap();
        print!("put_get_delete - After PUT function\n");

        let root = Path::from("/");

        // List everything
        let content_list = flatten_list_stream(storage, None).await.unwrap();
        assert_eq!(content_list, &[location.clone()]);

        // Should behave the same as no prefix
        let content_list = flatten_list_stream(storage, Some(&root)).await.unwrap();
        assert_eq!(content_list, &[location.clone()]);

        // List with delimiter
        let result = storage.list_with_delimiter(None).await.unwrap();
        assert_eq!(&result.objects, &[]);
        assert_eq!(result.common_prefixes.len(), 1);
        assert_eq!(result.common_prefixes[0], Path::from("test_dir"));

        // Should behave the same as no prefix
        let result = storage.list_with_delimiter(Some(&root)).await.unwrap();
        assert!(result.objects.is_empty());
        assert_eq!(result.common_prefixes.len(), 1);
        assert_eq!(result.common_prefixes[0], Path::from("test_dir"));

        // Should return not found
        let err = storage.get(&Path::from("test_dir")).await.unwrap_err();
        assert!(matches!(err, crate::Error::NotFound { .. }), "{}", err);

        // Should return not found
        let err = storage.head(&Path::from("test_dir")).await.unwrap_err();
        assert!(matches!(err, crate::Error::NotFound { .. }), "{}", err);

        // List everything starting with a prefix that should return results
        let prefix = Path::from("test_dir");
        let content_list = flatten_list_stream(storage, Some(&prefix)).await.unwrap();
        assert_eq!(content_list, &[location.clone()]);

        // List everything starting with a prefix that shouldn't return results
        let prefix = Path::from("something");
        let content_list = flatten_list_stream(storage, Some(&prefix)).await.unwrap();
        assert!(content_list.is_empty());

        let read_data = storage.get(&location).await.unwrap().bytes().await.unwrap();
        assert_eq!(&*read_data, data);

        // Test range request
        let range = 3..7;
        let range_result = storage.get_range(&location, range.clone()).await;

        let bytes = range_result.unwrap();
        assert_eq!(bytes, data.slice(range.clone()));

        let opts = GetOptions {
            range: Some(GetRange::Bounded(2..5)),
            ..Default::default()
        };
        let result = storage.get_opts(&location, opts).await.unwrap();
        // Data is `"arbitrary data"`, length 14 bytes
        assert_eq!(result.meta.size, 14); // Should return full object size (#5272)
        assert_eq!(result.range, 2..5);
        let bytes = result.bytes().await.unwrap();
        assert_eq!(bytes, b"bit".as_ref());

        let out_of_range = 200..300;
        let out_of_range_result = storage.get_range(&location, out_of_range).await;

        // Should be a non-fatal error
        out_of_range_result.unwrap_err();

        let opts = GetOptions {
            range: Some(GetRange::Bounded(2..100)),
            ..Default::default()
        };
        let result = storage.get_opts(&location, opts).await.unwrap();
        assert_eq!(result.range, 2..14);
        assert_eq!(result.meta.size, 14);
        let bytes = result.bytes().await.unwrap();
        assert_eq!(bytes, b"bitrary data".as_ref());

        let opts = GetOptions {
            range: Some(GetRange::Suffix(2)),
            ..Default::default()
        };
        match storage.get_opts(&location, opts).await {
            Ok(result) => {
                assert_eq!(result.range, 12..14);
                assert_eq!(result.meta.size, 14);
                let bytes = result.bytes().await.unwrap();
                assert_eq!(bytes, b"ta".as_ref());
            }
            Err(Error::NotSupported { .. }) => {}
            Err(e) => panic!("{e}"),
        }

        let opts = GetOptions {
            range: Some(GetRange::Suffix(100)),
            ..Default::default()
        };
        match storage.get_opts(&location, opts).await {
            Ok(result) => {
                assert_eq!(result.range, 0..14);
                assert_eq!(result.meta.size, 14);
                let bytes = result.bytes().await.unwrap();
                assert_eq!(bytes, b"arbitrary data".as_ref());
            }
            Err(Error::NotSupported { .. }) => {}
            Err(e) => panic!("{e}"),
        }

        let opts = GetOptions {
            range: Some(GetRange::Offset(3)),
            ..Default::default()
        };
        let result = storage.get_opts(&location, opts).await.unwrap();
        assert_eq!(result.range, 3..14);
        assert_eq!(result.meta.size, 14);
        let bytes = result.bytes().await.unwrap();
        assert_eq!(bytes, b"itrary data".as_ref());

        let opts = GetOptions {
            range: Some(GetRange::Offset(100)),
            ..Default::default()
        };
        storage.get_opts(&location, opts).await.unwrap_err();

        let ranges = vec![0..1, 2..3, 0..5];
        let bytes = storage.get_ranges(&location, &ranges).await.unwrap();
        for (range, bytes) in ranges.iter().zip(bytes) {
            assert_eq!(bytes, data.slice(range.clone()))
        }

        let head = storage.head(&location).await.unwrap();
        assert_eq!(head.size, data.len());

        storage.delete(&location).await.unwrap();

        let content_list = flatten_list_stream(storage, None).await.unwrap();
        assert!(content_list.is_empty());

        let err = storage.get(&location).await.unwrap_err();
        assert!(matches!(err, crate::Error::NotFound { .. }), "{}", err);

        let err = storage.head(&location).await.unwrap_err();
        assert!(matches!(err, crate::Error::NotFound { .. }), "{}", err);

        // Test handling of paths containing an encoded delimiter

        let file_with_delimiter = Path::from_iter(["a", "b/c", "foo.file"]);
        storage
            .put(&file_with_delimiter, "arbitrary".into())
            .await
            .unwrap();

        let files = flatten_list_stream(storage, None).await.unwrap();
        assert_eq!(files, vec![file_with_delimiter.clone()]);

        let files = flatten_list_stream(storage, Some(&Path::from("a/b")))
            .await
            .unwrap();
        assert!(files.is_empty());

        let files = storage
            .list_with_delimiter(Some(&Path::from("a/b")))
            .await
            .unwrap();
        assert!(files.common_prefixes.is_empty());
        assert!(files.objects.is_empty());

        let files = storage
            .list_with_delimiter(Some(&Path::from("a")))
            .await
            .unwrap();
        assert_eq!(files.common_prefixes, vec![Path::from_iter(["a", "b/c"])]);
        assert!(files.objects.is_empty());

        let files = storage
            .list_with_delimiter(Some(&Path::from_iter(["a", "b/c"])))
            .await
            .unwrap();
        assert!(files.common_prefixes.is_empty());
        assert_eq!(files.objects.len(), 1);
        assert_eq!(files.objects[0].location, file_with_delimiter);

        storage.delete(&file_with_delimiter).await.unwrap();

        // Test handling of paths containing non-ASCII characters, e.g. emoji

        let emoji_prefix = Path::from("🙀");
        let emoji_file = Path::from("🙀/😀.parquet");
        storage.put(&emoji_file, "arbitrary".into()).await.unwrap();

        storage.head(&emoji_file).await.unwrap();
        storage
            .get(&emoji_file)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();

        let files = flatten_list_stream(storage, Some(&emoji_prefix))
            .await
            .unwrap();

        assert_eq!(files, vec![emoji_file.clone()]);

        let dst = Path::from("foo.parquet");
        storage.copy(&emoji_file, &dst).await.unwrap();
        let mut files = flatten_list_stream(storage, None).await.unwrap();
        files.sort_unstable();
        assert_eq!(files, vec![emoji_file.clone(), dst.clone()]);

        let dst2 = Path::from("new/nested/foo.parquet");
        storage.copy(&emoji_file, &dst2).await.unwrap();
        let mut files = flatten_list_stream(storage, None).await.unwrap();
        files.sort_unstable();
        assert_eq!(files, vec![emoji_file.clone(), dst.clone(), dst2.clone()]);

        let dst3 = Path::from("new/nested2/bar.parquet");
        storage.rename(&dst, &dst3).await.unwrap();
        let mut files = flatten_list_stream(storage, None).await.unwrap();
        files.sort_unstable();
        assert_eq!(files, vec![emoji_file.clone(), dst2.clone(), dst3.clone()]);

        let err = storage.head(&dst).await.unwrap_err();
        assert!(matches!(err, Error::NotFound { .. }));

        storage.delete(&emoji_file).await.unwrap();
        storage.delete(&dst3).await.unwrap();
        storage.delete(&dst2).await.unwrap();
        let files = flatten_list_stream(storage, Some(&emoji_prefix))
            .await
            .unwrap();
        assert!(files.is_empty());

        // Test handling of paths containing percent-encoded sequences

        // "HELLO" percent encoded
        let hello_prefix = Path::parse("%48%45%4C%4C%4F").unwrap();
        let path = hello_prefix.child("foo.parquet");

        storage.put(&path, vec![0, 1].into()).await.unwrap();
        let files = flatten_list_stream(storage, Some(&hello_prefix))
            .await
            .unwrap();
        assert_eq!(files, vec![path.clone()]);

        // Cannot list by decoded representation
        let files = flatten_list_stream(storage, Some(&Path::from("HELLO")))
            .await
            .unwrap();
        assert!(files.is_empty());

        // Cannot access by decoded representation
        let err = storage
            .head(&Path::from("HELLO/foo.parquet"))
            .await
            .unwrap_err();
        assert!(matches!(err, crate::Error::NotFound { .. }), "{}", err);

        storage.delete(&path).await.unwrap();

        // Test handling of unicode paths
        let path = Path::parse("🇦🇺/$shenanigans@@~.txt").unwrap();
        storage.put(&path, "test".into()).await.unwrap();

        let r = storage.get(&path).await.unwrap();
        assert_eq!(r.bytes().await.unwrap(), "test");

        let dir = Path::parse("🇦🇺").unwrap();
        let r = storage.list_with_delimiter(None).await.unwrap();
        assert!(r.common_prefixes.contains(&dir));

        let r = storage.list_with_delimiter(Some(&dir)).await.unwrap();
        assert_eq!(r.objects.len(), 1);
        assert_eq!(r.objects[0].location, path);

        storage.delete(&path).await.unwrap();

        // Can also write non-percent encoded sequences
        let path = Path::parse("%Q.parquet").unwrap();
        storage.put(&path, vec![0, 1].into()).await.unwrap();

        let files = flatten_list_stream(storage, None).await.unwrap();
        assert_eq!(files, vec![path.clone()]);

        storage.delete(&path).await.unwrap();

        let path = Path::parse("foo bar/I contain spaces.parquet").unwrap();
        storage.put(&path, vec![0, 1].into()).await.unwrap();
        storage.head(&path).await.unwrap();

        let files = flatten_list_stream(storage, Some(&Path::from("foo bar")))
            .await
            .unwrap();
        assert_eq!(files, vec![path.clone()]);

        storage.delete(&path).await.unwrap();

        let files = flatten_list_stream(storage, None).await.unwrap();
        assert!(files.is_empty(), "{files:?}");

        // Test list order
        let files = vec![
            Path::from("a a/b.file"),
            Path::parse("a%2Fa.file").unwrap(),
            Path::from("a/😀.file"),
            Path::from("a/a file"),
            Path::parse("a/a%2F.file").unwrap(),
            Path::from("a/a.file"),
            Path::from("a/a/b.file"),
            Path::from("a/b.file"),
            Path::from("aa/a.file"),
            Path::from("ab/a.file"),
        ];

        for file in &files {
            storage.put(file, "foo".into()).await.unwrap();
        }

        let cases = [
            (None, Path::from("a")),
            (None, Path::from("a/a file")),
            (None, Path::from("a/a/b.file")),
            (None, Path::from("ab/a.file")),
            (None, Path::from("a%2Fa.file")),
            (None, Path::from("a/😀.file")),
            (Some(Path::from("a")), Path::from("")),
            (Some(Path::from("a")), Path::from("a")),
            (Some(Path::from("a")), Path::from("a/😀")),
            (Some(Path::from("a")), Path::from("a/😀.file")),
            (Some(Path::from("a")), Path::from("a/b")),
            (Some(Path::from("a")), Path::from("a/a/b.file")),
        ];

        for (prefix, offset) in cases {
            let s = storage.list_with_offset(prefix.as_ref(), &offset);
            let mut actual: Vec<_> = s.map_ok(|x| x.location).try_collect().await.unwrap();

            actual.sort_unstable();

            let expected: Vec<_> = files
                .iter()
                .filter(|x| {
                    let prefix_match = prefix.as_ref().map(|p| x.prefix_matches(p)).unwrap_or(true);
                    prefix_match && *x > &offset
                })
                .cloned()
                .collect();

            assert_eq!(actual, expected, "{prefix:?} - {offset:?}");
        }

        // Test bulk delete
        let paths = vec![
            Path::from("a/a.file"),
            Path::from("a/a/b.file"),
            Path::from("aa/a.file"),
            Path::from("does_not_exist"),
            Path::from("I'm a < & weird path"),
            Path::from("ab/a.file"),
            Path::from("a/😀.file"),
        ];

        storage.put(&paths[4], "foo".into()).await.unwrap();

        let out_paths = storage
            .delete_stream(futures::stream::iter(paths.clone()).map(Ok).boxed())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(out_paths.len(), paths.len());

        let expect_errors = [3];

        for (i, input_path) in paths.iter().enumerate() {
            let err = storage.head(input_path).await.unwrap_err();
            assert!(matches!(err, crate::Error::NotFound { .. }), "{}", err);

            if expect_errors.contains(&i) {
                // Some object stores will report NotFound, but others (such as S3) will
                // report success regardless.
                match &out_paths[i] {
                    Err(Error::NotFound { path: out_path, .. }) => {
                        assert!(out_path.ends_with(&input_path.to_string()));
                    }
                    Ok(out_path) => {
                        assert_eq!(out_path, input_path);
                    }
                    _ => panic!("unexpected error"),
                }
            } else {
                assert_eq!(out_paths[i].as_ref().unwrap(), input_path);
            }
        }

        delete_fixtures(storage).await;

        let path = Path::from("empty");
        storage.put(&path, PutPayload::default()).await.unwrap();
        let meta = storage.head(&path).await.unwrap();
        assert_eq!(meta.size, 0);
        let data = storage.get(&path).await.unwrap().bytes().await.unwrap();
        assert_eq!(data.len(), 0);

        storage.delete(&path).await.unwrap();
    }

    pub async fn put_get_attributes(integration: &dyn ObjectStore) {
        // Test handling of attributes
        let attributes = Attributes::from_iter([
            (Attribute::ContentType, "text/html; charset=utf-8"),
            (Attribute::CacheControl, "max-age=604800"),
        ]);

        let path = Path::from("attributes");
        let opts = attributes.clone().into();
        match integration.put_opts(&path, "foo".into(), opts).await {
            Ok(_) => {
                let r = integration.get(&path).await.unwrap();
                assert_eq!(r.attributes, attributes);
            }
            Err(Error::NotImplemented) => {}
            Err(e) => panic!("{e}"),
        }

        let opts = attributes.clone().into();
        match integration.put_multipart_opts(&path, opts).await {
            Ok(mut w) => {
                w.put_part("foo".into()).await.unwrap();
                w.complete().await.unwrap();

                let r = integration.get(&path).await.unwrap();
                assert_eq!(r.attributes, attributes);
            }
            Err(Error::NotImplemented) => {}
            Err(e) => panic!("{e}"),
        }
    }

    pub async fn get_opts(storage: &dyn ObjectStore) {
        let path = Path::from("test");
        storage.put(&path, "foo".into()).await.unwrap();
        let meta = storage.head(&path).await.unwrap();

        let options = GetOptions {
            if_unmodified_since: Some(meta.last_modified),
            ..GetOptions::default()
        };
        match storage.get_opts(&path, options).await {
            Ok(_) | Err(Error::NotSupported { .. }) => {}
            Err(e) => panic!("{e}"),
        }

        let options = GetOptions {
            if_unmodified_since: Some(
                meta.last_modified + chrono::Duration::try_hours(10).unwrap(),
            ),
            ..GetOptions::default()
        };
        match storage.get_opts(&path, options).await {
            Ok(_) | Err(Error::NotSupported { .. }) => {}
            Err(e) => panic!("{e}"),
        }

        let options = GetOptions {
            if_unmodified_since: Some(
                meta.last_modified - chrono::Duration::try_hours(10).unwrap(),
            ),
            ..GetOptions::default()
        };
        match storage.get_opts(&path, options).await {
            Err(Error::Precondition { .. } | Error::NotSupported { .. }) => {}
            d => panic!("{d:?}"),
        }

        let options = GetOptions {
            if_modified_since: Some(meta.last_modified),
            ..GetOptions::default()
        };
        match storage.get_opts(&path, options).await {
            Err(Error::NotModified { .. } | Error::NotSupported { .. }) => {}
            d => panic!("{d:?}"),
        }

        let options = GetOptions {
            if_modified_since: Some(meta.last_modified - chrono::Duration::try_hours(10).unwrap()),
            ..GetOptions::default()
        };
        match storage.get_opts(&path, options).await {
            Ok(_) | Err(Error::NotSupported { .. }) => {}
            Err(e) => panic!("{e}"),
        }

        let tag = meta.e_tag.unwrap();
        let options = GetOptions {
            if_match: Some(tag.clone()),
            ..GetOptions::default()
        };
        storage.get_opts(&path, options).await.unwrap();

        let options = GetOptions {
            if_match: Some("invalid".to_string()),
            ..GetOptions::default()
        };
        let err = storage.get_opts(&path, options).await.unwrap_err();
        assert!(matches!(err, Error::Precondition { .. }), "{err}");

        let options = GetOptions {
            if_none_match: Some(tag.clone()),
            ..GetOptions::default()
        };
        let err = storage.get_opts(&path, options).await.unwrap_err();
        assert!(matches!(err, Error::NotModified { .. }), "{err}");

        let options = GetOptions {
            if_none_match: Some("invalid".to_string()),
            ..GetOptions::default()
        };
        storage.get_opts(&path, options).await.unwrap();

        let result = storage.put(&path, "test".into()).await.unwrap();
        let new_tag = result.e_tag.unwrap();
        assert_ne!(tag, new_tag);

        let meta = storage.head(&path).await.unwrap();
        assert_eq!(meta.e_tag.unwrap(), new_tag);

        let options = GetOptions {
            if_match: Some(new_tag),
            ..GetOptions::default()
        };
        storage.get_opts(&path, options).await.unwrap();

        let options = GetOptions {
            if_match: Some(tag),
            ..GetOptions::default()
        };
        let err = storage.get_opts(&path, options).await.unwrap_err();
        assert!(matches!(err, Error::Precondition { .. }), "{err}");

        if let Some(version) = meta.version {
            storage.put(&path, "bar".into()).await.unwrap();

            let options = GetOptions {
                version: Some(version),
                ..GetOptions::default()
            };

            // Can retrieve previous version
            let get_opts = storage.get_opts(&path, options).await.unwrap();
            let old = get_opts.bytes().await.unwrap();
            assert_eq!(old, b"test".as_slice());

            // Current version contains the updated data
            let current = storage.get(&path).await.unwrap().bytes().await.unwrap();
            assert_eq!(&current, b"bar".as_slice());
        }
    }

    pub async fn put_opts(storage: &dyn ObjectStore, supports_update: bool) {
        // When using DynamoCommit repeated runs of this test will produce the same sequence of records in DynamoDB
        // As a result each conditional operation will need to wait for the lease to timeout before proceeding
        // One solution would be to clear DynamoDB before each test, but this would require non-trivial additional code
        // so we instead just generate a random suffix for the filenames
        let rng = thread_rng();
        let suffix = String::from_utf8(rng.sample_iter(Alphanumeric).take(32).collect()).unwrap();

        delete_fixtures(storage).await;
        let path = Path::from(format!("put_opts_{suffix}"));
        let v1 = storage
            .put_opts(&path, "a".into(), PutMode::Create.into())
            .await
            .unwrap();

        let err = storage
            .put_opts(&path, "b".into(), PutMode::Create.into())
            .await
            .unwrap_err();
        assert!(matches!(err, Error::AlreadyExists { .. }), "{err}");

        let b = storage.get(&path).await.unwrap().bytes().await.unwrap();
        assert_eq!(b.as_ref(), b"a");

        if !supports_update {
            return;
        }

        let v2 = storage
            .put_opts(&path, "c".into(), PutMode::Update(v1.clone().into()).into())
            .await
            .unwrap();

        let b = storage.get(&path).await.unwrap().bytes().await.unwrap();
        assert_eq!(b.as_ref(), b"c");

        let err = storage
            .put_opts(&path, "d".into(), PutMode::Update(v1.into()).into())
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Precondition { .. }), "{err}");

        storage
            .put_opts(&path, "e".into(), PutMode::Update(v2.clone().into()).into())
            .await
            .unwrap();

        let b = storage.get(&path).await.unwrap().bytes().await.unwrap();
        assert_eq!(b.as_ref(), b"e");

        // Update not exists
        let path = Path::from("I don't exist");
        let err = storage
            .put_opts(&path, "e".into(), PutMode::Update(v2.into()).into())
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Precondition { .. }), "{err}");

        const NUM_WORKERS: usize = 5;
        const NUM_INCREMENTS: usize = 10;

        let path = Path::from(format!("RACE-{suffix}"));
        let mut futures: FuturesUnordered<_> = (0..NUM_WORKERS)
            .map(|_| async {
                for _ in 0..NUM_INCREMENTS {
                    loop {
                        match storage.get(&path).await {
                            Ok(r) => {
                                let mode = PutMode::Update(UpdateVersion {
                                    e_tag: r.meta.e_tag.clone(),
                                    version: r.meta.version.clone(),
                                });

                                let b = r.bytes().await.unwrap();
                                let v: usize = std::str::from_utf8(&b).unwrap().parse().unwrap();
                                let new = (v + 1).to_string();

                                match storage.put_opts(&path, new.into(), mode.into()).await {
                                    Ok(_) => break,
                                    Err(Error::Precondition { .. }) => continue,
                                    Err(e) => return Err(e),
                                }
                            }
                            Err(Error::NotFound { .. }) => {
                                let mode = PutMode::Create;
                                match storage.put_opts(&path, "1".into(), mode.into()).await {
                                    Ok(_) => break,
                                    Err(Error::AlreadyExists { .. }) => continue,
                                    Err(e) => return Err(e),
                                }
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }
                Ok(())
            })
            .collect();

        while futures.next().await.transpose().unwrap().is_some() {}
        let b = storage.get(&path).await.unwrap().bytes().await.unwrap();
        let v = std::str::from_utf8(&b).unwrap().parse::<usize>().unwrap();
        assert_eq!(v, NUM_WORKERS * NUM_INCREMENTS);
    }

    /// Returns a chunk of length `chunk_length`
    fn get_chunk(chunk_length: usize) -> Bytes {
        let mut data = vec![0_u8; chunk_length];
        let mut rng = thread_rng();
        // Set a random selection of bytes
        for _ in 0..1000 {
            data[rng.gen_range(0..chunk_length)] = rng.gen();
        }
        data.into()
    }

    /// Returns `num_chunks` of length `chunks`
    fn get_chunks(chunk_length: usize, num_chunks: usize) -> Vec<Bytes> {
        (0..num_chunks).map(|_| get_chunk(chunk_length)).collect()
    }

    pub async fn stream_get(storage: &DynObjectStore) {
        let location = Path::from("test_dir/test_upload_file.txt");

        // Can write to storage
        let data = get_chunks(5 * 1024 * 1024, 3);
        let bytes_expected = data.concat();
        let mut upload = storage.put_multipart(&location).await.unwrap();
        let uploads = data.into_iter().map(|x| upload.put_part(x.into()));
        futures::future::try_join_all(uploads).await.unwrap();

        // Object should not yet exist in store
        let meta_res = storage.head(&location).await;
        assert!(meta_res.is_err());
        assert!(matches!(
            meta_res.unwrap_err(),
            crate::Error::NotFound { .. }
        ));

        let files = flatten_list_stream(storage, None).await.unwrap();
        assert_eq!(&files, &[]);

        let result = storage.list_with_delimiter(None).await.unwrap();
        assert_eq!(&result.objects, &[]);

        upload.complete().await.unwrap();

        let bytes_written = storage.get(&location).await.unwrap().bytes().await.unwrap();
        assert_eq!(bytes_expected, bytes_written);

        // Can overwrite some storage
        // Sizes chosen to ensure we write three parts
        let data = get_chunks(3_200_000, 7);
        let bytes_expected = data.concat();
        let upload = storage.put_multipart(&location).await.unwrap();
        let mut writer = WriteMultipart::new(upload);
        for chunk in &data {
            writer.write(chunk)
        }
        writer.finish().await.unwrap();
        let bytes_written = storage.get(&location).await.unwrap().bytes().await.unwrap();
        assert_eq!(bytes_expected, bytes_written);

        // We can abort an empty write
        let location = Path::from("test_dir/test_abort_upload.txt");
        let mut upload = storage.put_multipart(&location).await.unwrap();
        upload.abort().await.unwrap();
        let get_res = storage.get(&location).await;
        assert!(get_res.is_err());
        assert!(matches!(
            get_res.unwrap_err(),
            crate::Error::NotFound { .. }
        ));

        // We can abort an in-progress write
        let mut upload = storage.put_multipart(&location).await.unwrap();
        upload
            .put_part(data.first().unwrap().clone().into())
            .await
            .unwrap();

        upload.abort().await.unwrap();
        let get_res = storage.get(&location).await;
        assert!(get_res.is_err());
        assert!(matches!(
            get_res.unwrap_err(),
            crate::Error::NotFound { .. }
        ));
    }

    pub async fn list_uses_directories_correctly(storage: &DynObjectStore) {
        delete_fixtures(storage).await;

        let content_list = flatten_list_stream(storage, None).await.unwrap();
        assert!(
            content_list.is_empty(),
            "Expected list to be empty; found: {content_list:?}"
        );

        let location1 = Path::from("foo/x.json");
        let location2 = Path::from("foo.bar/y.json");

        let data = PutPayload::from("arbitrary data");
        storage.put(&location1, data.clone()).await.unwrap();
        storage.put(&location2, data).await.unwrap();

        let prefix = Path::from("foo");
        let content_list = flatten_list_stream(storage, Some(&prefix)).await.unwrap();
        assert_eq!(content_list, &[location1.clone()]);

        let result = storage.list_with_delimiter(Some(&prefix)).await.unwrap();
        assert_eq!(result.objects.len(), 1);
        assert_eq!(result.objects[0].location, location1);
        assert_eq!(result.common_prefixes, &[]);

        // Listing an existing path (file) should return an empty list:
        // https://github.com/apache/arrow-rs/issues/3712
        let content_list = flatten_list_stream(storage, Some(&location1))
            .await
            .unwrap();
        assert_eq!(content_list, &[]);

        let list = storage.list_with_delimiter(Some(&location1)).await.unwrap();
        assert_eq!(list.objects, &[]);
        assert_eq!(list.common_prefixes, &[]);

        let prefix = Path::from("foo/x");
        let content_list = flatten_list_stream(storage, Some(&prefix)).await.unwrap();
        assert_eq!(content_list, &[]);

        let list = storage.list_with_delimiter(Some(&prefix)).await.unwrap();
        assert_eq!(list.objects, &[]);
        assert_eq!(list.common_prefixes, &[]);
    }

    pub async fn list_with_delimiter(storage: &DynObjectStore) {
        delete_fixtures(storage).await;

        // ==================== check: store is empty ====================
        let content_list = flatten_list_stream(storage, None).await.unwrap();
        assert!(content_list.is_empty());

        // ==================== do: create files ====================
        let data = Bytes::from("arbitrary data");

        let files: Vec<_> = [
            "test_file",
            "mydb/wb/000/000/000.segment",
            "mydb/wb/000/000/001.segment",
            "mydb/wb/000/000/002.segment",
            "mydb/wb/001/001/000.segment",
            "mydb/wb/foo.json",
            "mydb/wbwbwb/111/222/333.segment",
            "mydb/data/whatevs",
        ]
        .iter()
        .map(|&s| Path::from(s))
        .collect();

        for f in &files {
            storage.put(f, data.clone().into()).await.unwrap();
        }

        // ==================== check: prefix-list `mydb/wb` (directory) ====================
        let prefix = Path::from("mydb/wb");

        let expected_000 = Path::from("mydb/wb/000");
        let expected_001 = Path::from("mydb/wb/001");
        let expected_location = Path::from("mydb/wb/foo.json");

        let result = storage.list_with_delimiter(Some(&prefix)).await.unwrap();

        assert_eq!(result.common_prefixes, vec![expected_000, expected_001]);
        assert_eq!(result.objects.len(), 1);

        let object = &result.objects[0];

        assert_eq!(object.location, expected_location);
        assert_eq!(object.size, data.len());

        // ==================== check: prefix-list `mydb/wb/000/000/001` (partial filename doesn't match) ====================
        let prefix = Path::from("mydb/wb/000/000/001");

        let result = storage.list_with_delimiter(Some(&prefix)).await.unwrap();
        assert!(result.common_prefixes.is_empty());
        assert_eq!(result.objects.len(), 0);

        // ==================== check: prefix-list `not_there` (non-existing prefix) ====================
        let prefix = Path::from("not_there");

        let result = storage.list_with_delimiter(Some(&prefix)).await.unwrap();
        assert!(result.common_prefixes.is_empty());
        assert!(result.objects.is_empty());

        // ==================== do: remove all files ====================
        for f in &files {
            storage.delete(f).await.unwrap();
        }

        // ==================== check: store is empty ====================
        let content_list = flatten_list_stream(storage, None).await.unwrap();
        assert!(content_list.is_empty());
    }

    pub(crate) async fn get_nonexistent_object(
        storage: &DynObjectStore,
        location: Option<Path>,
    ) -> crate::Result<Bytes> {
        let location = location.unwrap_or_else(|| Path::from("this_file_should_not_exist"));

        let err = storage.head(&location).await.unwrap_err();
        assert!(matches!(err, crate::Error::NotFound { .. }));

        storage.get(&location).await?.bytes().await
    }

    pub async fn rename_and_copy(storage: &DynObjectStore) {
        // Create two objects
        let path1 = Path::from("test1");
        let path2 = Path::from("test2");
        let contents1 = Bytes::from("cats");
        let contents2 = Bytes::from("dogs");

        // copy() make both objects identical
        storage.put(&path1, contents1.clone().into()).await.unwrap();
        storage.put(&path2, contents2.clone().into()).await.unwrap();
        storage.copy(&path1, &path2).await.unwrap();
        let new_contents = storage.get(&path2).await.unwrap().bytes().await.unwrap();
        assert_eq!(&new_contents, &contents1);

        // rename() copies contents and deletes original
        storage.put(&path1, contents1.clone().into()).await.unwrap();
        storage.put(&path2, contents2.clone().into()).await.unwrap();
        storage.rename(&path1, &path2).await.unwrap();
        let new_contents = storage.get(&path2).await.unwrap().bytes().await.unwrap();
        assert_eq!(&new_contents, &contents1);
        let result = storage.get(&path1).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), crate::Error::NotFound { .. }));

        // Clean up
        storage.delete(&path2).await.unwrap();
    }

    pub async fn copy_if_not_exists(storage: &DynObjectStore) {
        // Create two objects
        let path1 = Path::from("test1");
        let path2 = Path::from("not_exists_nested/test2");
        let contents1 = Bytes::from("cats");
        let contents2 = Bytes::from("dogs");

        // copy_if_not_exists() errors if destination already exists
        storage.put(&path1, contents1.clone().into()).await.unwrap();
        storage.put(&path2, contents2.clone().into()).await.unwrap();
        let result = storage.copy_if_not_exists(&path1, &path2).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            crate::Error::AlreadyExists { .. }
        ));

        // copy_if_not_exists() copies contents and allows deleting original
        storage.delete(&path2).await.unwrap();
        storage.copy_if_not_exists(&path1, &path2).await.unwrap();
        storage.delete(&path1).await.unwrap();
        let new_contents = storage.get(&path2).await.unwrap().bytes().await.unwrap();
        assert_eq!(&new_contents, &contents1);
        let result = storage.get(&path1).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), crate::Error::NotFound { .. }));

        // Clean up
        storage.delete(&path2).await.unwrap();
    }

    pub async fn copy_rename_nonexistent_object(storage: &DynObjectStore) {
        // Create empty source object
        let path1 = Path::from("test1");

        // Create destination object
        let path2 = Path::from("test2");
        storage.put(&path2, "hello".into()).await.unwrap();

        // copy() errors if source does not exist
        let result = storage.copy(&path1, &path2).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), crate::Error::NotFound { .. }));

        // rename() errors if source does not exist
        let result = storage.rename(&path1, &path2).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), crate::Error::NotFound { .. }));

        // copy_if_not_exists() errors if source does not exist
        let result = storage.copy_if_not_exists(&path1, &path2).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), crate::Error::NotFound { .. }));

        // Clean up
        storage.delete(&path2).await.unwrap();
    }

    pub async fn multipart(storage: &dyn ObjectStore, multipart: &dyn MultipartStore) {
        let path = Path::from("test_multipart");
        let chunk_size = 5 * 1024 * 1024;

        let chunks = get_chunks(chunk_size, 2);

        let id = multipart.create_multipart(&path).await.unwrap();

        let parts: Vec<_> = futures::stream::iter(chunks)
            .enumerate()
            .map(|(idx, b)| multipart.put_part(&path, &id, idx, b.into()))
            .buffered(2)
            .try_collect()
            .await
            .unwrap();

        multipart
            .complete_multipart(&path, &id, parts)
            .await
            .unwrap();

        let meta = storage.head(&path).await.unwrap();
        assert_eq!(meta.size, chunk_size * 2);

        // Empty case
        let path = Path::from("test_empty_multipart");

        let id = multipart.create_multipart(&path).await.unwrap();

        let parts = vec![];

        multipart
            .complete_multipart(&path, &id, parts)
            .await
            .unwrap();

        let meta = storage.head(&path).await.unwrap();
        assert_eq!(meta.size, 0);
    }

    async fn delete_fixtures(storage: &DynObjectStore) {
        let paths = storage.list(None).map_ok(|meta| meta.location).boxed();
        storage
            .delete_stream(paths)
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
    }

    /// Test that the returned stream does not borrow the lifetime of Path
    fn list_store<'a>(
        store: &'a dyn ObjectStore,
        path_str: &str,
    ) -> BoxStream<'a, Result<ObjectMeta>> {
        let path = Path::from(path_str);
        store.list(Some(&path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_util::*;

    #[tokio::test]
    async fn file_test() {
        let integration = HadoopFileSystem::new();

        put_get_delete_list(&integration).await;
        get_opts(&integration).await; 
        list_uses_directories_correctly(&integration).await;
        list_with_delimiter(&integration).await;
        rename_and_copy(&integration).await;
        copy_if_not_exists(&integration).await;
        copy_rename_nonexistent_object(&integration).await;
        stream_get(&integration).await;
        put_opts(&integration, false).await;
    }


}
