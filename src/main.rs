use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use tokio::{
    fs,
    sync::{mpsc, oneshot},
    task,
};

#[derive(Debug)]
struct DirRequest {
    pub dir_name: String,
    pub sender: oneshot::Sender<String>,
    pub close: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut tasks: Vec<_> = vec![];
    // Get the path to the directory to sort. -- TODO: This will come in as a CLI arg.
    let the_path = Path::new("./sort_me");

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
                    dir_name,
                    sender,
                    close: false,
                } => {
                    // If the dir name isn't in the existing_dirs set:
                    // check if the dir exists.
                    println!("got a request! {:?}", &dir_name);
                    let _ = sender.send(String::from(&dir_name));
                    existing_dirs.insert(dir_name);
                }
                DirRequest { close: true, .. } => {
                    println!("here's all of the dirs. {:?}", existing_dirs);
                    rx.close();
                }
            }
        }
    });

    // Get the contents of the junk drawer directory as a stream.
    let mut dir_stream = fs::read_dir(the_path).await?;

    // With each item from the stream
    while let Some(dir_entry) = dir_stream.next_entry().await? {
        println!("processing dir entry!!! YAY! {:?}", dir_entry);
        let ttx = tx.clone();
        let t = task::spawn(async move {
            let dir_name = dir_entry.file_name().into_string().unwrap();
            let (req_tx, req_rx) = oneshot::channel();

            let dir_request = DirRequest {
                dir_name,
                sender: req_tx,
                close: false,
            };

            ttx.send(dir_request).await.unwrap();

            // Send a dir request to the dir creator task.
            // Move the file to the target dir.
            let res = req_rx
                .await
                .expect("failed to receive data from dir manager");

            println!("got response! {}", res);
        });

        tasks.push(t);
    }

    println!("Waiting for tasks...");
    for t in tasks {
        t.await?;
    }

    // use the original mpsc transmitter to send the shutdown request
    // to the dir_manager task so it closes the channel.

    // TODO: Make the sender optional so we don't have waste cycles
    // just to create a DirRequest.
    let (_tx, _rx) = oneshot::channel();

    tx.send(DirRequest {
        dir_name: "".into(),
        sender: _tx,
        close: true,
    })
    .await?;

    println!("Waiting for dir manager.");

    dir_manager.await?;

    println!("done!");

    Ok(())
}
