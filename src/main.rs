use std::{
    fs::File,
    io::{Cursor, Read, Seek, Write},
    process::Command,
};

use tempdir::TempDir;
use walkdir::WalkDir;
use warp::Filter;

fn unzip_and_get_command<R: Read + Seek>(mut zip: zip::ZipArchive<R>) -> Option<String> {
    let mut args = String::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).ok()?;
        let filename = String::from(file.name());
        if filename == "args" {
            file.read_to_string(&mut args).ok()?;
            args = args.trim().to_string();
            println!("found args: {}", args);
        } else {
            println!("extract: {}", filename);
            std::io::copy(&mut file, &mut File::create(filename).ok()?).ok()?;
        }
    }
    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

fn execute(command: String) -> Option<(Vec<u8>, Vec<u8>)> {
    let output = Command::new("sh").arg("-c").arg(command).output().ok()?;

    Some((output.stdout, output.stderr))
}

fn pack_all(stdout: &[u8], stderr: &[u8]) -> Option<Vec<u8>> {
    let writer = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(writer);
    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o644);

    for file in WalkDir::new(".").into_iter().filter_map(Result::ok) {
        let filename = file.path();
        dbg!(filename);
        if file.file_type().is_file() {
            if let Ok(mut file) = File::open(filename) {
                zip.start_file(filename.to_str()?.to_string(), options)
                    .ok()?;
                std::io::copy(&mut file, &mut zip).ok()?;
            }
        } else if file.file_type().is_dir() {
            zip.add_directory(filename.to_str()?.to_string(), options)
                .ok();
        }
    }

    zip.start_file("stdout", options).ok()?;
    zip.write_all(&stdout).ok()?;
    zip.start_file("stderr", options).ok()?;
    zip.write_all(&stderr).ok()?;

    Some(zip.finish().ok()?.into_inner())
}

fn doall<R: Read + Seek>(zip: zip::ZipArchive<R>) -> Option<Vec<u8>> {
    let _dir = if std::env::var("SERVE_TMPDIR").is_ok() {
        let dir = TempDir::new("serve").ok()?;
        dbg!(dir.path());
        std::env::set_current_dir(dir.path()).ok()?;
        Some(dir)
    } else {
        None
    };

    let command = unzip_and_get_command(zip)?;
    let (stdout, stderr) = execute(command)?;
    let output = pack_all(&stdout, &stderr)?;

    Some(output)
}

#[tokio::main]
async fn main() {
    let hello = warp::post()
        .and(warp::body::bytes())
        .map(|data: warp::hyper::body::Bytes| {
            let reader = Cursor::new(data);
            let zip = zip::ZipArchive::new(reader).unwrap();

            if let Some(out) = doall(zip) {
                out
            } else {
                "couldn't run the thing".as_bytes().to_vec()
            }
        });

    println!("starting server on port 3030");
    warp::serve(hello).run(([0, 0, 0, 0], 3030)).await;
}
