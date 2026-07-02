use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ark_core::GameOutcome;
use ark_model::{game_from_uci, read_replay, write_replay, GameRecord};
use ark_replay::{
    chunk_path, deterministic_chunk_id, read_chunk, read_chunked_replay_dir, read_replay_any,
    rebuild_manifest, temporary_chunk_path, validate_records, write_chunk_atomic,
    ChunkWriteOptions,
};

#[test]
fn chunk_roundtrip_seals_tmp_and_validates_records() -> Result<(), String> {
    let dir = temp_dir("ark-replay-v2-roundtrip")?;
    let games = sample_games()?;

    let manifest = write_chunk_atomic(
        &dir,
        ChunkWriteOptions {
            first_game_index: 7,
        },
        &games,
    )?;

    assert_eq!(manifest.schema_version, 2);
    assert_eq!(manifest.games, 3);
    assert_eq!(manifest.plies, 10);
    assert_eq!(manifest.white_wins, 1);
    assert_eq!(manifest.draws, 1);
    assert_eq!(manifest.black_wins, 1);
    assert_eq!(
        manifest.chunk_id,
        deterministic_chunk_id(7, &games),
        "chunk id must be deterministic from index and content"
    );

    let sealed = chunk_path(&dir, &manifest.chunk_id);
    assert!(sealed.exists());
    assert!(!temporary_chunk_path(&sealed).exists());

    let chunk = read_chunk(&sealed)?;
    assert_eq!(chunk.manifest, manifest);
    assert_eq!(chunk.games, games);
    let validation = validate_records(&chunk.games)?;
    assert_eq!(validation.illegal_moves, 0);
    assert_eq!(validation.unhandled_terminal_states, 0);

    cleanup_dir(&dir)
}

#[test]
fn manifest_rebuild_sorts_chunks_and_summarizes_counts() -> Result<(), String> {
    let dir = temp_dir("ark-replay-v2-manifest")?;
    let games = sample_games()?;
    let first = write_chunk_atomic(
        &dir,
        ChunkWriteOptions {
            first_game_index: 0,
        },
        &games[..1],
    )?;
    let second = write_chunk_atomic(
        &dir,
        ChunkWriteOptions {
            first_game_index: 1,
        },
        &games[1..],
    )?;

    let manifest = rebuild_manifest(&dir)?;
    assert_eq!(manifest.schema_version, 2);
    assert_eq!(manifest.chunks, vec![first.clone(), second.clone()]);
    assert_eq!(manifest.games, first.games + second.games);
    assert_eq!(manifest.plies, first.plies + second.plies);
    assert_eq!(manifest.white_wins, first.white_wins + second.white_wins);
    assert_eq!(manifest.draws, first.draws + second.draws);
    assert_eq!(manifest.black_wins, first.black_wins + second.black_wins);
    assert_eq!(
        manifest.sealed_bytes,
        first.sealed_bytes + second.sealed_bytes
    );

    cleanup_dir(&dir)
}

#[test]
fn chunked_replay_dir_reads_games_in_game_index_order() -> Result<(), String> {
    let run_dir = temp_dir("ark-replay-v2-read-dir")?;
    let chunks_dir = run_dir.join("chunks");
    let games = sample_games()?;
    write_chunk_atomic(
        &chunks_dir,
        ChunkWriteOptions {
            first_game_index: 1,
        },
        &games[1..],
    )?;
    write_chunk_atomic(
        &chunks_dir,
        ChunkWriteOptions {
            first_game_index: 0,
        },
        &games[..1],
    )?;

    assert_eq!(read_chunked_replay_dir(&chunks_dir)?, games);
    assert_eq!(read_replay_any(&chunks_dir)?, games);
    assert_eq!(read_replay_any(&run_dir)?, games);

    cleanup_dir(&run_dir)
}

#[test]
fn chunked_replay_dir_rejects_missing_game_index() -> Result<(), String> {
    let dir = temp_dir("ark-replay-v2-read-gap")?;
    let games = sample_games()?;
    write_chunk_atomic(
        &dir,
        ChunkWriteOptions {
            first_game_index: 1,
        },
        &games[..1],
    )?;

    let err = read_chunked_replay_dir(&dir)
        .err()
        .ok_or("chunked replay with missing game index unexpectedly read")?;
    assert!(
        err.contains("expected first_game_index=0"),
        "unexpected gap error: {err}"
    );

    cleanup_dir(&dir)
}

#[test]
fn corrupt_and_truncated_chunks_are_rejected() -> Result<(), String> {
    let dir = temp_dir("ark-replay-v2-corrupt")?;
    let games = sample_games()?;
    let manifest = write_chunk_atomic(
        &dir,
        ChunkWriteOptions {
            first_game_index: 42,
        },
        &games,
    )?;
    let sealed = chunk_path(&dir, &manifest.chunk_id);
    let bytes = fs::read(sealed).map_err(|err| err.to_string())?;

    let corrupt = dir.join("corrupt.arkchunk");
    let mut corrupt_bytes = bytes.clone();
    if corrupt_bytes.is_empty() {
        return Err("sealed chunk unexpectedly empty".to_string());
    }
    let last = corrupt_bytes.len() - 1;
    corrupt_bytes[last] ^= 0x55;
    fs::write(&corrupt, corrupt_bytes).map_err(|err| err.to_string())?;
    let corrupt_err = read_chunk(&corrupt)
        .err()
        .ok_or("corrupt chunk unexpectedly read")?;
    assert!(
        corrupt_err.contains("chunk content hash mismatch")
            || corrupt_err.contains("chunk manifest mismatch")
            || corrupt_err.contains("chunk validation failed"),
        "unexpected corrupt error: {corrupt_err}"
    );

    let truncated = dir.join("truncated.arkchunk");
    fs::write(&truncated, &bytes[..bytes.len() - 1]).map_err(|err| err.to_string())?;
    let truncated_err = read_chunk(&truncated)
        .err()
        .ok_or("truncated chunk unexpectedly read")?;
    assert!(
        truncated_err.contains("sealed byte length mismatch")
            || truncated_err.contains("failed to read"),
        "unexpected truncated error: {truncated_err}"
    );

    cleanup_dir(&dir)
}

#[test]
fn arkgames_v1_roundtrip_stays_compatible() -> Result<(), String> {
    let dir = temp_dir("ark-replay-v1-compat")?;
    let path = dir.join("compat.arkgames");
    let games = sample_games()?;

    let summary = write_replay(&path, &games)?;
    assert_eq!(summary.games, 3);
    assert_eq!(summary.plies, 10);
    assert_eq!(read_replay(&path)?, games);
    assert_eq!(read_replay_any(&path)?, games);

    cleanup_dir(&dir)
}

fn sample_games() -> Result<Vec<GameRecord>, String> {
    Ok(vec![
        game_from_uci(
            GameOutcome::WhiteWin,
            &uci_moves(&["e2e4", "e7e5", "d1h5", "b8c6"]),
        )?,
        game_from_uci(GameOutcome::Draw, &uci_moves(&["g1f3", "d7d5", "g2g3"]))?,
        game_from_uci(GameOutcome::BlackWin, &uci_moves(&["d2d4", "d7d5", "c1f4"]))?,
    ])
}

fn uci_moves(moves: &[&str]) -> Vec<String> {
    moves.iter().map(|mv| String::from(*mv)).collect()
}

fn temp_dir(name: &str) -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()));
    if dir.exists() {
        cleanup_dir(&dir)?;
    }
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

fn cleanup_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|err| err.to_string())?;
    }
    Ok(())
}
