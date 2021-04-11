use anyhow::{anyhow, Result};
use clap::{App, Arg};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::{
    fs::{self, DirBuilder},
    sync::{mpsc, oneshot},
    task,
};

#[derive(Debug)]
struct DirRequest {
    pub dir_path: PathBuf,
    pub sender: oneshot::Sender<PathBuf>, // TODO: Make this a Result<PathBuf>
    pub close: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set up CLI.
    let matches = App::new("Tidy")
        .version("0.1.0")
        .author("nilz3ro")
        .about("clean it up!")
        .arg(
            Arg::with_name("dir")
                .help("the directory to tidy")
                .index(1)
                .required(true),
        )
        .arg(Arg::with_name("outdir").short("o").help(
            "the directory where the sorted files will be stored in subdirectories named after their file extension.",
        ).index(2).required(false))
        .get_matches();

    let dir = matches.value_of("dir").unwrap();
    let out = matches.value_of("outdir").unwrap_or("./sorted");
    // TODO: verify source_dir exists!
    let source_dir = Path::new(dir).to_path_buf();
    let target_root_dir = Path::new(out).to_path_buf();

    let mut tasks = vec![];

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
    let dir_manager = task::spawn(async move {
        // The set of confirmed existing dirs,
        // this is used to track which target dirs that have been
        // confirmed.
        let mut existing_dirs = HashSet::new();
        let mut dir_builder = DirBuilder::new();

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
                            println!(
                                "dir not confirmed: {:?}, checking for existence.",
                                &dir_path
                            );
                            if dir_path.exists() {
                                println!("The dir exists! adding it to the set.");
                                existing_dirs.insert(dir_path.clone());
                                let _ = sender.send(dir_path);
                            } else {
                                println!("The dir does not exist. Creating {:?}.", &dir_path);
                                match dir_builder.recursive(true).create(dir_path.clone()).await {
                                    Ok(_) => {
                                        println!("We created the dir.");
                                        existing_dirs.insert(dir_path.clone());
                                        let _ = sender.send(dir_path);
                                    }
                                    Err(e) => {
                                        println!("Something went wrong while creating the dir.");
                                        eprintln!("{:?}", e);
                                        let _ = sender.send(dir_path);
                                    }
                                }
                            }
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
        // Ignore directories for now.
        if dir_entry.path().is_dir() {
            continue;
        }

        let required_dir_path = target_dir_for_extension(&target_root_dir, dir_entry.path()).await;

        // Ignore files with no extension for now.
        if required_dir_path.is_none() {
            continue;
        }

        let ttx = tx.clone();

        let t = task::spawn(async move {
            let dir_path = required_dir_path.unwrap();
            let (req_tx, req_rx) = oneshot::channel();

            let dir_request = DirRequest {
                dir_path: dir_path.clone(),
                sender: req_tx,
                close: false,
            };

            ttx.send(dir_request).await.unwrap();

            // Send a dir request to the dir creator task.
            // Move the file to the target dir.
            let res = req_rx
                .await
                .expect("failed to receive data from dir manager");

            println!("got response! {:?}", res);

            // TODO: Res is the target path.
            // Perform file copying here...
            match fs::copy(
                dir_entry.path(),
                dir_path.clone().join(dir_entry.file_name()),
            )
            .await
            {
                Ok(_) => {
                    println!("Moved the file!");
                }
                Err(e) => {
                    println!(
                        "Could not move: {:?} to {:?}",
                        dir_entry.file_name(),
                        dir_path
                    );
                    eprintln!("{:?}", e);
                }
            };
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

// Finds the target directory based on file extension.
// If the path does not have a file extension, it will return `None`.
async fn target_dir_for_extension(target: &Path, path: PathBuf) -> Option<PathBuf> {
    match path.extension() {
        Some(ext) => {
            let p = Path::new(ext).to_owned();
            Some(target.join(p).to_path_buf())
        }
        None => None,
    }
}
