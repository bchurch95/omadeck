# CURRENT TASK: Embedded Audio & Video Playback (Milestone 9)

## Goal
Support playing slide media parts (ppt/media/*.mp4, .wav, .mp3) with auto-play on slide entry, looping, and pause controls in both the editor canvas and presenter console.

## Context
- Milestone 10 is now 100% COMPLETE!
- Shape::Media(m) exists in office_toolkit::powerpoint.
- inspect.rs already has a Shape::Media(m) arm returning kind "media".

## Steps
1. In crates/omashow-core/src/inspect.rs:
   - Add MediaInfo struct with media_type ("video" or "audio") and data_uri.
   - Attach media: Option<MediaInfo> to ShapeInfo.
2. In apps/omashow-tauri/src/frontend/slide_render.js:
   - When sh.kind === "media", render a positioned <video> or <audio> element.
3. Tests & Verification:
   - Run cargo test -p omashow-core.
   - Mark [x] Embedded Audio & Video Playback in TODO.md.
   - Commit: feat(media): add audio/video playback support
