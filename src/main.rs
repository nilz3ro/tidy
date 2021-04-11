use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use tokio::{
    fs,
    sync::{mpsc, oneshot},
    task,
};

#[derive(Debug)]
struct DirRequest {
    pub dir_path: PathBuf,
    pub sender: oneshot::Sender<PathBuf>,
    pub close: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut tasks = vec![];
    // The source directory - the directory that we want to sort.
    // TODO: get this as a cli arg.
    let source_dir = Path::new("./sort_me").to_path_buf();
    // In the future we will support sorting strategies, but for now
    // the only way to sort will be by file extension.
    //
    // So, we will need to know what the target dir for each extension is, and if it exists.
    // If it doesn't exist, the dir_manager task can create it as requested.
    //
    // For now we will only support copying files to target directories that share a common root.
    // In the future, the user will be able to configure the target directory for each collection in a sorting strategy.
    //
    // TODO:  get this as a cli arg.
    let target_root_dir = Path::new("./sorted");

    let (tx, mut rx) = mpsc::channel(48);

    // Make a dir creator task that takes requests from the other file mover
    // tasks and creates a dir for the file type if it doesn't exist.
    //
    // It should always respond with Ok(Path) if the target dir exists.
    // this is how the mover tasks will know when the file can be moved.
    //
    // The mover tasks will send the dir creator task a DirRequest which will have a
    // name field and a transmitter field.
    //
    // The dir creator will check to see if a directory
    // the dir_request.name exists in its internal HashSet and fire off an Result<()> through the
    // transmitter end (a oneshot channel).
    //
    // TODO: code here.
    let dir_manager = task::spawn(async move {
        // The set of confirmed existing dirs,
        // this is used to track which target dirs that have been
        // confirmed.
        let mut existing_dirs = HashSet::new();

        while let Some(dir_request) = rx.recv().await {
            match dir_request {
                DirRequest {
                    dir_path,
                    sender,
                    close: false,
                } => {
                    // 1. Try to find the path in the set.
                    // 2. If the path is in the set, respond with the Ok(path) immediately.
                    // 3. If the path is not in the set,
                    // 3.1 Find out if the path exists, and confirm that it's a directory.
                    // 3.2 If the path exists and it's a directory, add it to the set.
                    // 3.3 If the path exists and it's not a directory. respond with Err.
                    // 3.4 If the path does not exist, create it and add it to the set,
                    // then respond with the Ok(path).
                    //
                    // TODO: Flatten the above conditions within one match
                    // statement using a tuple (known, exists).
                    match existing_dirs.contains(&dir_path) {
                        true => {
                            println!("found dir: {:?}", &dir_path);
                            let _ = sender.send(dir_path);
                        }
                        false => {
                            // create the dir.
                            println!("dir {:?} not found. creating.", &dir_path);
                            existing_dirs.insert(dir_path.clone());
                            let _ = sender.send(dir_path);
                        }
                    }
                }
                DirRequest { close: true, .. } => {
                    // Perform shutdown task.
                    rx.close();
                }
            }
        }
    });

    // Get the contents of the junk drawer directory as a stream.
    let mut dir_stream = fs::read_dir(source_dir).await?;

    // With each item from the stream
    while let Some(dir_entry) = dir_stream.next_entry().await? {
        let required_dir_path = target_dir_for_extension(&target_root_dir, dir_entry.path()).await;

        if required_dir_path.is_none() {
            continue;
        }

        let ttx = tx.clone();

        let t = task::spawn(async move {
            let dir_path = required_dir_path.unwrap();
            let (req_tx, req_rx) = oneshot::channel();

            let dir_request = DirRequest {
                dir_path,
                sender: req_tx,
                close: false,
            };

            ttx.send(dir_request).await.unwrap();

            // Send a dir request to the dir creator task.
            // Move the file to the target dir.
            let res = req_rx
                .await
                .expect("failed to receive data from dir manager");

            // TODO: Res is the target path.
            // Perform file copying here...

            println!("got response! {:?}", res);
        });

        tasks.push(t);
    }

    // TODO: Enable log levels. Find a good logger crate.
    // println!("Waiting for tasks...");
    for t in tasks {
        t.await?;
    }

    // TODO: Make the sender Option<Sender<PathBuf>> so we don't have waste cycles
    // just to create a DirRequest.
    let (_tx, _rx) = oneshot::channel();

    // use the original mpsc transmitter to send the shutdown request
    // to the dir_manager task so it closes the channel.
    tx.send(DirRequest {
        dir_path: "".into(),
        sender: _tx,
        close: true,
    })
    .await?;

    // println!("Waiting for dir manager.");

    dir_manager.await?;

    println!("done!");

    Ok(())
}

async fn target_dir_for_extension(tg: &Path, pth: PathBuf) -> Option<PathBuf> {
    match pth.extension() {
        Some(ext) => {
            let p = Path::new(ext).to_owned();
            Some(tg.join(p).to_path_buf())
        }
        None => None,
    }
}
