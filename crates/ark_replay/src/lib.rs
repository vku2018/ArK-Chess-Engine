use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ark_core::GameOutcome;
use ark_model::{read_replay as read_legacy_replay, validate_replay, GameRecord, ReplayValidation};

const CHUNK_MAGIC: &[u8; 8] = b"ARKC4V2\0";
const CHUNK_VERSION: u32 = 2;
const CHUNK_EXTENSION: &str = "arkchunk";
const MAX_CHUNK_ID_BYTES: usize = 64;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub type ReplayChunkResult<T> = Result<T, String>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkWriteOptions {
    pub first_game_index: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkManifest {
    pub schema_version: u32,
    pub chunk_id: String,
    pub first_game_index: u64,
    pub games: u32,
    pub plies: u64,
    pub white_wins: u32,
    pub draws: u32,
    pub black_wins: u32,
    pub validation_illegal_moves: u32,
    pub validation_unhandled_terminal_states: u32,
    pub content_hash: u64,
    pub body_bytes: u64,
    pub sealed_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayChunk {
    pub manifest: ChunkManifest,
    pub games: Vec<GameRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayManifest {
    pub schema_version: u32,
    pub chunks: Vec<ChunkManifest>,
    pub games: u32,
    pub plies: u64,
    pub white_wins: u32,
    pub draws: u32,
    pub black_wins: u32,
    pub sealed_bytes: u64,
}

pub fn deterministic_chunk_id(first_game_index: u64, games: &[GameRecord]) -> String {
    format!(
        "arkv2-{first_game_index:016x}-{:08x}-{:016x}",
        games.len(),
        hash_records(games)
    )
}

pub fn chunk_path(dir: &Path, chunk_id: &str) -> PathBuf {
    dir.join(format!("{chunk_id}.{CHUNK_EXTENSION}"))
}

pub fn temporary_chunk_path(sealed_path: &Path) -> PathBuf {
    let mut tmp = sealed_path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

pub fn validate_records(games: &[GameRecord]) -> ReplayChunkResult<ReplayValidation> {
    validate_replay(games)
}

/// Reads either a legacy `.arkgames` replay file or an `.arkchunk` replay directory.
///
/// Chunked self-play writes `out/chunks/*.arkchunk`; callers may pass either the
/// `chunks` directory itself or the run directory containing it.
pub fn read_replay_any(path: &Path) -> ReplayChunkResult<Vec<GameRecord>> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("failed to stat replay path {}: {err}", path.display()))?;
    if metadata.is_file() {
        return read_legacy_replay(path)
            .map_err(|err| format!("failed to read legacy replay {}: {err}", path.display()));
    }
    if metadata.is_dir() {
        return read_chunked_replay_any_dir(path);
    }
    Err(format!(
        "replay path is neither a file nor a directory: {}",
        path.display()
    ))
}

/// Reads a directory of sealed `.arkchunk` files into deterministic game order.
pub fn read_chunked_replay_dir(dir: &Path) -> ReplayChunkResult<Vec<GameRecord>> {
    let paths = chunk_file_paths(dir)?;
    if paths.is_empty() {
        return Err(format!(
            "chunked replay directory contains no .{CHUNK_EXTENSION} files: {}",
            dir.display()
        ));
    }

    let mut chunks = Vec::with_capacity(paths.len());
    for path in paths {
        let chunk = read_chunk(&path)?;
        validate_chunk_filename(&path, &chunk.manifest)?;
        chunks.push(chunk);
    }
    chunks.sort_by(|left, right| {
        left.manifest
            .first_game_index
            .cmp(&right.manifest.first_game_index)
            .then_with(|| left.manifest.chunk_id.cmp(&right.manifest.chunk_id))
    });

    let mut expected_first_game_index = 0_u64;
    let mut games = Vec::new();
    for chunk in chunks {
        if chunk.manifest.first_game_index != expected_first_game_index {
            return Err(format!(
                "chunk index gap or overlap in {}: expected first_game_index={}, got {} for {}",
                dir.display(),
                expected_first_game_index,
                chunk.manifest.first_game_index,
                chunk.manifest.chunk_id
            ));
        }
        let decoded_games = u32::try_from(chunk.games.len()).map_err(|_err| {
            format!(
                "decoded chunk game count exceeds u32 limit for {}",
                chunk.manifest.chunk_id
            )
        })?;
        if decoded_games != chunk.manifest.games {
            return Err(format!(
                "chunk decoded game count mismatch for {}: manifest={}, decoded={decoded_games}",
                chunk.manifest.chunk_id, chunk.manifest.games
            ));
        }
        if decoded_games == 0 {
            return Err(format!(
                "chunk contains zero games: {}",
                chunk.manifest.chunk_id
            ));
        }
        expected_first_game_index = checked_add(
            expected_first_game_index,
            u64::from(decoded_games),
            "replay game index",
        )?;
        games.extend(chunk.games);
    }

    let validation = validate_records(&games)?;
    if validation.illegal_moves != 0 || validation.unhandled_terminal_states != 0 {
        return Err(format!(
            "chunked replay validation failed: illegal_moves={}, unhandled_terminal_states={}",
            validation.illegal_moves, validation.unhandled_terminal_states
        ));
    }

    Ok(games)
}

pub fn write_chunk_atomic(
    dir: &Path,
    options: ChunkWriteOptions,
    games: &[GameRecord],
) -> ReplayChunkResult<ChunkManifest> {
    fs::create_dir_all(dir).map_err(|err| format!("failed to create chunk dir: {err}"))?;

    let validation = validate_records(games)?;
    if validation.illegal_moves != 0 || validation.unhandled_terminal_states != 0 {
        return Err(format!(
            "replay validation failed before chunk write: illegal_moves={}, unhandled_terminal_states={}",
            validation.illegal_moves, validation.unhandled_terminal_states
        ));
    }

    let manifest = build_manifest(options.first_game_index, games, validation)?;
    let sealed_path = chunk_path(dir, &manifest.chunk_id);
    if sealed_path.exists() {
        let existing = read_chunk(&sealed_path)?;
        if existing.manifest == manifest && existing.games == games {
            return Ok(existing.manifest);
        }
        return Err(format!(
            "sealed chunk already exists with different content: {}",
            sealed_path.display()
        ));
    }

    let tmp_path = temporary_chunk_path(&sealed_path);
    if tmp_path.exists() {
        fs::remove_file(&tmp_path).map_err(|err| {
            format!(
                "failed to remove stale chunk temp file {}: {err}",
                tmp_path.display()
            )
        })?;
    }

    if let Err(err) = write_chunk_file(&tmp_path, &manifest, games) {
        return Err(cleanup_tmp_after_error(&tmp_path, err));
    }
    if let Err(err) = fs::rename(&tmp_path, &sealed_path) {
        return Err(cleanup_tmp_after_error(
            &tmp_path,
            format!(
                "failed to seal chunk {} from temp {}: {err}",
                sealed_path.display(),
                tmp_path.display()
            ),
        ));
    }

    let sealed = read_chunk(&sealed_path)?;
    if sealed.manifest != manifest || sealed.games != games {
        return Err(format!(
            "sealed chunk verification mismatch: {}",
            sealed_path.display()
        ));
    }
    Ok(sealed.manifest)
}

pub fn read_chunk(path: &Path) -> ReplayChunkResult<ReplayChunk> {
    let mut file = File::open(path)
        .map_err(|err| format!("failed to open chunk {}: {err}", path.display()))?;
    let file_len = file
        .metadata()
        .map_err(|err| format!("failed to stat chunk {}: {err}", path.display()))?
        .len();
    let manifest = read_manifest_header(&mut file)?;
    if file_len != manifest.sealed_bytes {
        return Err(format!(
            "sealed byte length mismatch for {}: header={}, file={}",
            path.display(),
            manifest.sealed_bytes,
            file_len
        ));
    }

    let mut games = Vec::with_capacity(manifest.games as usize);
    let mut body_bytes = 0_u64;
    for _ in 0..manifest.games {
        let result = read_result(&mut file)?;
        body_bytes = checked_add(body_bytes, 1, "chunk body byte count")?;
        let plies = read_u16(&mut file, "game ply count")?;
        body_bytes = checked_add(body_bytes, 2, "chunk body byte count")?;
        let mut moves = Vec::with_capacity(usize::from(plies));
        for _ in 0..plies {
            moves.push(read_u16(&mut file, "packed move")?);
            body_bytes = checked_add(body_bytes, 2, "chunk body byte count")?;
        }
        games.push(GameRecord { result, moves });
    }

    if body_bytes != manifest.body_bytes {
        return Err(format!(
            "chunk body length mismatch: header={}, decoded={body_bytes}",
            manifest.body_bytes
        ));
    }

    let mut extra = [0_u8; 1];
    let trailing = file
        .read(&mut extra)
        .map_err(|err| format!("failed to check chunk trailing bytes: {err}"))?;
    if trailing != 0 {
        return Err("chunk has trailing bytes after decoded body".to_string());
    }

    let actual_hash = hash_records(&games);
    if actual_hash != manifest.content_hash {
        return Err(format!(
            "chunk content hash mismatch: header={:016x}, body={actual_hash:016x}",
            manifest.content_hash
        ));
    }

    let validation = validate_records(&games)?;
    let actual_manifest = build_manifest(manifest.first_game_index, &games, validation)?;
    if actual_manifest != manifest {
        return Err(format!(
            "chunk manifest mismatch: header chunk_id={}, decoded chunk_id={}",
            manifest.chunk_id, actual_manifest.chunk_id
        ));
    }
    if validation.illegal_moves != 0 || validation.unhandled_terminal_states != 0 {
        return Err(format!(
            "chunk validation failed: illegal_moves={}, unhandled_terminal_states={}",
            validation.illegal_moves, validation.unhandled_terminal_states
        ));
    }

    Ok(ReplayChunk { manifest, games })
}

fn read_chunked_replay_any_dir(path: &Path) -> ReplayChunkResult<Vec<GameRecord>> {
    if !chunk_file_paths(path)?.is_empty() {
        return read_chunked_replay_dir(path);
    }

    let nested = path.join("chunks");
    if nested.is_dir() && !chunk_file_paths(&nested)?.is_empty() {
        return read_chunked_replay_dir(&nested);
    }

    Err(format!(
        "replay directory contains no .{CHUNK_EXTENSION} files in {} or {}",
        path.display(),
        nested.display()
    ))
}

fn chunk_file_paths(dir: &Path) -> ReplayChunkResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)
        .map_err(|err| format!("failed to read chunk dir {}: {err}", dir.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read chunk dir entry: {err}"))?;
        let file_type = entry
            .file_type()
            .map_err(|err| format!("failed to read chunk dir entry type: {err}"))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension() == Some(OsStr::new(CHUNK_EXTENSION)) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn validate_chunk_filename(path: &Path, manifest: &ChunkManifest) -> ReplayChunkResult<()> {
    let stem = path
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("bad chunk filename: {}", path.display()))?;
    if stem != manifest.chunk_id {
        return Err(format!(
            "chunk filename does not match manifest id for {}: filename={}, manifest={}",
            path.display(),
            stem,
            manifest.chunk_id
        ));
    }
    Ok(())
}

pub fn rebuild_manifest(dir: &Path) -> ReplayChunkResult<ReplayManifest> {
    let mut chunks = Vec::new();
    for entry in fs::read_dir(dir)
        .map_err(|err| format!("failed to read chunk dir {}: {err}", dir.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read chunk dir entry: {err}"))?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new(CHUNK_EXTENSION)) {
            continue;
        }
        chunks.push(read_chunk(&path)?.manifest);
    }

    chunks.sort_by(|left, right| {
        left.first_game_index
            .cmp(&right.first_game_index)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });

    let mut manifest = ReplayManifest {
        schema_version: CHUNK_VERSION,
        chunks,
        games: 0,
        plies: 0,
        white_wins: 0,
        draws: 0,
        black_wins: 0,
        sealed_bytes: 0,
    };

    for chunk in &manifest.chunks {
        manifest.games = checked_add_u32(manifest.games, chunk.games, "manifest game count")?;
        manifest.plies = checked_add(manifest.plies, chunk.plies, "manifest ply count")?;
        manifest.white_wins =
            checked_add_u32(manifest.white_wins, chunk.white_wins, "manifest white wins")?;
        manifest.draws = checked_add_u32(manifest.draws, chunk.draws, "manifest draws")?;
        manifest.black_wins =
            checked_add_u32(manifest.black_wins, chunk.black_wins, "manifest black wins")?;
        manifest.sealed_bytes = checked_add(
            manifest.sealed_bytes,
            chunk.sealed_bytes,
            "manifest sealed bytes",
        )?;
    }

    Ok(manifest)
}

fn write_chunk_file(
    path: &Path,
    manifest: &ChunkManifest,
    games: &[GameRecord],
) -> ReplayChunkResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| format!("failed to create chunk temp file {}: {err}", path.display()))?;

    write_manifest_header(&mut file, manifest)?;
    for game in games {
        file.write_all(&[result_byte(game.result)])
            .map_err(|err| format!("failed to write game result: {err}"))?;
        write_u16(
            &mut file,
            checked_len_u16(game.moves.len(), "game ply count")?,
        )?;
        for mv in &game.moves {
            write_u16(&mut file, *mv)?;
        }
    }
    file.flush()
        .map_err(|err| format!("failed to flush chunk temp file {}: {err}", path.display()))?;
    file.sync_all()
        .map_err(|err| format!("failed to sync chunk temp file {}: {err}", path.display()))?;
    Ok(())
}

fn write_manifest_header(file: &mut File, manifest: &ChunkManifest) -> ReplayChunkResult<()> {
    let chunk_id_bytes = manifest.chunk_id.as_bytes();
    let chunk_id_len = checked_len_u16(chunk_id_bytes.len(), "chunk id length")?;
    file.write_all(CHUNK_MAGIC)
        .map_err(|err| format!("failed to write chunk magic: {err}"))?;
    write_u32(file, manifest.schema_version)?;
    write_u16(file, chunk_id_len)?;
    file.write_all(chunk_id_bytes)
        .map_err(|err| format!("failed to write chunk id: {err}"))?;
    write_u64(file, manifest.first_game_index)?;
    write_u32(file, manifest.games)?;
    write_u64(file, manifest.plies)?;
    write_u32(file, manifest.white_wins)?;
    write_u32(file, manifest.draws)?;
    write_u32(file, manifest.black_wins)?;
    write_u32(file, manifest.validation_illegal_moves)?;
    write_u32(file, manifest.validation_unhandled_terminal_states)?;
    write_u64(file, manifest.content_hash)?;
    write_u64(file, manifest.body_bytes)?;
    write_u64(file, manifest.sealed_bytes)?;
    Ok(())
}

fn read_manifest_header(file: &mut File) -> ReplayChunkResult<ChunkManifest> {
    let mut magic = [0_u8; 8];
    file.read_exact(&mut magic)
        .map_err(|err| format!("failed to read chunk magic: {err}"))?;
    if &magic != CHUNK_MAGIC {
        return Err("bad ArK replay chunk magic".to_string());
    }

    let schema_version = read_u32(file, "chunk version")?;
    if schema_version != CHUNK_VERSION {
        return Err(format!(
            "unsupported ArK replay chunk version: {schema_version}"
        ));
    }

    let chunk_id_len = usize::from(read_u16(file, "chunk id length")?);
    if chunk_id_len == 0 || chunk_id_len > MAX_CHUNK_ID_BYTES {
        return Err(format!("bad chunk id length: {chunk_id_len}"));
    }
    let mut chunk_id = vec![0_u8; chunk_id_len];
    file.read_exact(&mut chunk_id)
        .map_err(|err| format!("failed to read chunk id: {err}"))?;
    let chunk_id =
        String::from_utf8(chunk_id).map_err(|err| format!("bad chunk id utf8: {err}"))?;

    Ok(ChunkManifest {
        schema_version,
        chunk_id,
        first_game_index: read_u64(file, "first game index")?,
        games: read_u32(file, "game count")?,
        plies: read_u64(file, "ply count")?,
        white_wins: read_u32(file, "white win count")?,
        draws: read_u32(file, "draw count")?,
        black_wins: read_u32(file, "black win count")?,
        validation_illegal_moves: read_u32(file, "validation illegal move count")?,
        validation_unhandled_terminal_states: read_u32(
            file,
            "validation unhandled terminal state count",
        )?,
        content_hash: read_u64(file, "content hash")?,
        body_bytes: read_u64(file, "body byte count")?,
        sealed_bytes: read_u64(file, "sealed byte count")?,
    })
}

fn build_manifest(
    first_game_index: u64,
    games: &[GameRecord],
    validation: ReplayValidation,
) -> ReplayChunkResult<ChunkManifest> {
    let game_count = checked_len_u32(games.len(), "chunk game count")?;
    let chunk_id = deterministic_chunk_id(first_game_index, games);
    if chunk_id.len() > MAX_CHUNK_ID_BYTES {
        return Err(format!("chunk id too long: {}", chunk_id.len()));
    }

    let mut plies = 0_u64;
    let mut body_bytes = 0_u64;
    let mut white_wins = 0_u32;
    let mut draws = 0_u32;
    let mut black_wins = 0_u32;
    for game in games {
        let move_count = checked_len_u16(game.moves.len(), "game ply count")?;
        plies = checked_add(plies, u64::from(move_count), "chunk ply count")?;
        body_bytes = checked_add(body_bytes, 3, "chunk body byte count")?;
        body_bytes = checked_add(
            body_bytes,
            u64::from(move_count) * 2,
            "chunk body byte count",
        )?;
        match game.result {
            GameOutcome::WhiteWin => white_wins = checked_add_u32(white_wins, 1, "white wins")?,
            GameOutcome::Draw => draws = checked_add_u32(draws, 1, "draws")?,
            GameOutcome::BlackWin => black_wins = checked_add_u32(black_wins, 1, "black wins")?,
        }
    }

    if validation.games != game_count || validation.plies != plies {
        return Err(format!(
            "validation summary mismatch: validation_games={}, manifest_games={}, validation_plies={}, manifest_plies={plies}",
            validation.games, game_count, validation.plies
        ));
    }

    let header_bytes = header_len(&chunk_id)?;
    let sealed_bytes = checked_add(header_bytes, body_bytes, "sealed byte count")?;
    Ok(ChunkManifest {
        schema_version: CHUNK_VERSION,
        chunk_id,
        first_game_index,
        games: game_count,
        plies,
        white_wins,
        draws,
        black_wins,
        validation_illegal_moves: validation.illegal_moves,
        validation_unhandled_terminal_states: validation.unhandled_terminal_states,
        content_hash: hash_records(games),
        body_bytes,
        sealed_bytes,
    })
}

fn header_len(chunk_id: &str) -> ReplayChunkResult<u64> {
    let id_len = checked_len_u16(chunk_id.len(), "chunk id length")?;
    Ok(8 + 4 + 2 + u64::from(id_len) + 8 + 4 + 8 + 4 + 4 + 4 + 4 + 4 + 8 + 8 + 8)
}

fn hash_records(games: &[GameRecord]) -> u64 {
    let mut hash = FNV_OFFSET;
    feed_u32(
        &mut hash,
        checked_len_u32(games.len(), "chunk game count").unwrap_or(u32::MAX),
    );
    for game in games {
        feed_u8(&mut hash, result_byte(game.result));
        feed_u16(
            &mut hash,
            checked_len_u16(game.moves.len(), "game ply count").unwrap_or(u16::MAX),
        );
        for mv in &game.moves {
            feed_u16(&mut hash, *mv);
        }
    }
    hash
}

fn feed_u8(hash: &mut u64, value: u8) {
    *hash ^= u64::from(value);
    *hash = hash.wrapping_mul(FNV_PRIME);
}

fn feed_u16(hash: &mut u64, value: u16) {
    for byte in value.to_le_bytes() {
        feed_u8(hash, byte);
    }
}

fn feed_u32(hash: &mut u64, value: u32) {
    for byte in value.to_le_bytes() {
        feed_u8(hash, byte);
    }
}

fn result_byte(result: GameOutcome) -> u8 {
    result.white_score().to_le_bytes()[0]
}

fn read_result(file: &mut File) -> ReplayChunkResult<GameOutcome> {
    let byte = read_u8(file, "game result")?;
    match i8::from_le_bytes([byte]) {
        1 => Ok(GameOutcome::WhiteWin),
        0 => Ok(GameOutcome::Draw),
        -1 => Ok(GameOutcome::BlackWin),
        other => Err(format!("bad replay chunk result: {other}")),
    }
}

fn write_u16(file: &mut File, value: u16) -> ReplayChunkResult<()> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| format!("failed to write u16: {err}"))
}

fn write_u32(file: &mut File, value: u32) -> ReplayChunkResult<()> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| format!("failed to write u32: {err}"))
}

fn write_u64(file: &mut File, value: u64) -> ReplayChunkResult<()> {
    file.write_all(&value.to_le_bytes())
        .map_err(|err| format!("failed to write u64: {err}"))
}

fn read_u8(file: &mut File, name: &str) -> ReplayChunkResult<u8> {
    let mut bytes = [0_u8; 1];
    file.read_exact(&mut bytes)
        .map_err(|err| format!("failed to read {name}: {err}"))?;
    Ok(bytes[0])
}

fn read_u16(file: &mut File, name: &str) -> ReplayChunkResult<u16> {
    let mut bytes = [0_u8; 2];
    file.read_exact(&mut bytes)
        .map_err(|err| format!("failed to read {name}: {err}"))?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(file: &mut File, name: &str) -> ReplayChunkResult<u32> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes)
        .map_err(|err| format!("failed to read {name}: {err}"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(file: &mut File, name: &str) -> ReplayChunkResult<u64> {
    let mut bytes = [0_u8; 8];
    file.read_exact(&mut bytes)
        .map_err(|err| format!("failed to read {name}: {err}"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn checked_len_u16(len: usize, name: &str) -> ReplayChunkResult<u16> {
    u16::try_from(len).map_err(|_| format!("{name} exceeds u16 limit: {len}"))
}

fn checked_len_u32(len: usize, name: &str) -> ReplayChunkResult<u32> {
    u32::try_from(len).map_err(|_| format!("{name} exceeds u32 limit: {len}"))
}

fn checked_add(left: u64, right: u64, name: &str) -> ReplayChunkResult<u64> {
    left.checked_add(right)
        .ok_or_else(|| format!("{name} overflow: {left} + {right}"))
}

fn checked_add_u32(left: u32, right: u32, name: &str) -> ReplayChunkResult<u32> {
    left.checked_add(right)
        .ok_or_else(|| format!("{name} overflow: {left} + {right}"))
}

fn cleanup_tmp_after_error(tmp_path: &Path, cause: String) -> String {
    match fs::remove_file(tmp_path) {
        Ok(()) => cause,
        Err(cleanup_err) => format!(
            "{cause}; also failed to remove chunk temp file {}: {cleanup_err}",
            tmp_path.display()
        ),
    }
}
