#![warn(clippy::all, clippy::pedantic)]

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use async_openai::{
    types::{
        ChatCompletionRequestMessageContentPartImageArgs,
        ChatCompletionRequestMessageContentPartTextArgs, ChatCompletionRequestUserMessageArgs,
        ChatCompletionRequestUserMessageContent, CreateChatCompletionRequestArgs,
    },
    Client,
};
use base64::prelude::*;
use chrono::{DateTime, Local};
use clap::Parser;
use dialoguer::Confirm;
use exif::{In, Reader as ExifReader, Tag};
use figment::{
    providers::{Format, Json},
    Figment,
};
use fs_err as fs;
use indicatif::{ProgressBar, ProgressStyle};
use infer::Infer;
use walkdir::WalkDir;

const REVERT_PATH: &str = "revert-mappings.json";

type RevertMappings = BTreeMap<String, String>;

#[derive(Debug, Clone, Parser)]
#[clap(author, version, about, long_about = None, arg_required_else_help = true)]
pub struct Cli {
    #[clap(short, long, help = "Prompt to rename or revert each file")]
    prompt: bool,

    #[clap(short, long, help = "Revert file(s) to the original name(s)")]
    revert: bool,

    #[clap(required = false)]
    paths: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    ctrlc::set_handler(|| {
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let args = Cli::parse();
    let revert_path = data_path()?;

    let mut revert_mappings: RevertMappings =
        Figment::new().merge(Json::file(&revert_path)).extract()?;

    if args.revert {
        revert_filenames(&args, &mut revert_mappings, &revert_path)?;
    } else {
        rename_files(&args, &mut revert_mappings, &revert_path).await?;
    };

    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn rename_files(
    args: &Cli,
    revert_mappings: &mut RevertMappings,
    revert_path: &Path,
) -> Result<()> {
    let client = Client::new();
    let infer = Infer::new();

    let mut paths: Vec<PathBuf> = vec![];

    for path in &args.paths {
        //
        if path.is_file() && is_image_file(path, &infer) {
            paths.push(path.clone());
        } else if path.is_dir() {
            for entry in WalkDir::new(path)
                .into_iter()
                .filter_map(std::result::Result::ok)
                .filter(|e| e.path().is_file() && is_image_file(e.path(), &infer))
            {
                paths.push(entry.into_path());
            }
        } else {
            println!("The path {path:?} is not a valid file or directory");
        }
    }

    println!("Processing {} images...", paths.len());

    for filename in &paths {
        let s = spinner();

        s.set_message(format!("⊙ Generating new filename for: {filename:?} ..."));

        let mut file = File::open(filename)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let encoded = BASE64_STANDARD.encode(&buffer);

        // Grab either the EXIF date or the file creation date
        let date_instructions = match file_date(filename) {
           Some(date) => format!("If the original filename doesn't contain date information, use this date instead: {date}"),
            _ => String::new()
        };

        let mut content = vec![];

        content.push(ChatCompletionRequestMessageContentPartTextArgs::default()
                .text(format!("Return a filename that describes this image, including the extension and optionally the date information from
                       the original name: {filename:?} in the format of YYYY-MM-DD at the beginning of the filename.

                       {date_instructions}

                       The words in the filename should be capitlized.

                       The filename should use dashes to separate words and should not include any special characters.

                       The filename should be no more than 64 characters long, not including the date information.")
            ) .build()?.into()
        );

        content.push(
            ChatCompletionRequestMessageContentPartImageArgs::default()
                .image_url(format!("data:image/jpeg;base64,{encoded}"))
                .build()?
                .into(),
        );

        let request = CreateChatCompletionRequestArgs::default()
            .model("gpt-4-vision-preview")
            .messages([ChatCompletionRequestUserMessageArgs::default()
                .content(ChatCompletionRequestUserMessageContent::Array(content))
                .build()?
                .into()])
            .max_tokens(300_u16)
            .build()?;

        let response = client.chat().create(request).await?;

        let new_path = if let Some(choice) = response.choices.first() {
            if let Some(text) = &choice.message.content {
                let new_path = filename.with_file_name(text);

                if new_path.is_file() {
                    s.finish_with_message(format!(
                        "Filename already exists, skipping file: {filename:?}"
                    ));
                    continue;
                }

                new_path
            } else {
                s.finish_with_message(format!(
                    "No response from OpenAI, skipping file: {filename:?}"
                ));
                continue;
            }
        } else {
            s.finish_with_message(format!(
                "No response from OpenAI, skipping file: {filename:?}"
            ));

            continue;
        };

        s.finish_and_clear();

        let rename = if args.prompt {
            Confirm::new()
                .with_prompt(format!("Will rename {filename:?} to: {new_path:?} ok?"))
                .default(true)
                .show_default(false)
                .wait_for_newline(true)
                .interact()
                .expect("Failed to read input")
        } else {
            true
        };

        if rename {
            println!("Renaming {filename:?} to: {new_path:?}");

            fs::rename(filename, &new_path).expect("Failed to rename file");
        }

        revert_mappings.insert(
            new_path.to_string_lossy().into_owned(),
            filename.to_string_lossy().into_owned(),
        );
    }

    write_revert_mappings(revert_path, revert_mappings)
}

fn revert_filenames(
    args: &Cli,
    revert_mappings: &mut RevertMappings,
    revert_path: &Path,
) -> Result<()> {
    //
    let to_revert: Vec<String> = if args.paths.is_empty() {
        revert_mappings.keys().cloned().collect()
    } else {
        args.paths
            .iter()
            .filter_map(|path| revert_mappings.get(&path.to_string_lossy().to_string()))
            .cloned()
            .collect()
    };

    for new_path in to_revert {
        //
        if !PathBuf::from(&new_path).exists() {
            println!("The file {new_path:?} does not exist anymore! Skipping");
            continue;
        }

        let original_path = revert_mappings.get(&new_path).unwrap();

        let revert = if args.prompt {
            Confirm::new()
                .with_prompt(format!(
                    "Will revert {new_path:?} to: {original_path:?} ok?"
                ))
                .default(true)
                .show_default(false)
                .wait_for_newline(true)
                .interact()
                .expect("Failed to read input")
        } else {
            true
        };

        if revert {
            println!("Reverting {new_path:?} to: {original_path:?}");

            fs::rename(&new_path, original_path).expect("Failed to rename file");

            revert_mappings.remove(&new_path);
        }
    }

    write_revert_mappings(revert_path, revert_mappings)
}

fn data_path() -> Result<PathBuf> {
    let xdg_dir =
        xdg::BaseDirectories::with_prefix("image-renamer").context("Failed get data directory")?;

    xdg_dir.place_data_file(REVERT_PATH).map_err(Into::into)
}

fn is_image_file(path: &Path, infer: &Infer) -> bool {
    if let Ok(mut file) = File::open(path) {
        let mut buffer = vec![0; 4096]; // Read up to 4096 bytes

        if file.read(&mut buffer).is_ok() {
            if let Some(kind) = infer.get(&buffer) {
                return kind.mime_type().starts_with("image/");
            }
        }
    }

    false
}

fn file_date(path: &Path) -> Option<String> {
    exif_date(path).or_else(|| stat_date(path))
}

fn stat_date(path: &Path) -> Option<String> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.created().ok())
        .map(|ctime| {
            let datetime: DateTime<Local> = ctime.into();
            datetime.format("Y-m-d").to_string()
        })
}

fn exif_date(path: &Path) -> Option<String> {
    let fh = File::open(path).expect("Couldn't open '{file}' in read-only mode");

    let Ok(exif) = ExifReader::new().read_from_container(&mut BufReader::new(&fh)) else {
        return None;
    };

    exif.get_field(Tag::DateTimeOriginal, In::PRIMARY)
        .map(|field| field.value.display_as(field.tag).to_string())
}

fn write_revert_mappings(revert_path: &Path, revert_mappings: &RevertMappings) -> Result<()> {
    let file = File::create(revert_path)?;

    serde_json::to_writer_pretty(file, &revert_mappings).map_err(Into::into)
}

fn spinner() -> ProgressBar {
    let pb = ProgressBar::new_spinner();

    pb.enable_steady_tick(Duration::from_millis(50));

    pb.set_style(
        ProgressStyle::with_template("{msg} {spinner:.cyan.bold}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
    );

    pb
}
