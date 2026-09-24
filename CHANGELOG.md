# Changelog

All notable changes to OverLingo are documented in this file.

## [1.2.0](https://github.com/Deanwfy/OverLingo/compare/v1.1.0...v1.2.0) - 2026-09-24

### Features

- Let older subtitles run off the top of the overlay
- Float the overlay's route labels over the subtitles
- Float the toolbar and settings above the subtitle box (#1)
- Make the subtitle size slider continuous and the text larger
- Keep the subtitle box where it was left
- Bring the subtitle box back at launch if it was showing at quit
- Check for and install updates from inside the app

### Bug Fixes

- Make selected segments and disabled route cards read correctly in dark mode
- Bring back the edit shortcuts on macOS
- Leave the translator unchosen until the user picks one
- Keep the audio source row where apps cannot be captured
- Show translator errors as sent and let a failed session be retried
- Start in the tray on Windows too

### Chore

- Stop uploading the macOS app archive
- Add the landing page and record the demo
- Name the publisher for Windows bundles
- Register the macOS login item through SMAppService
- Lay the overlay display settings out two by two
- Require macOS 14.5
- Fold the split overlay columns along a single hairline
- Call the overlay background slider opacity, not transparency
- Check out every text file with LF on all platforms
- Give the system audio route the accent and the microphone the green
- Let the translator and history pages grow with the window
- Round the subtitle window itself on macOS

## [1.1.0](https://github.com/Deanwfy/OverLingo/compare/v1.0.0...v1.1.0) - 2026-08-31

### Features

- Add a merged subtitle layout that interleaves both routes
- Choose which microphone to capture from

### Maintenance

- Sign macOS dev builds through a cargo runner
- Generate the changelog with git-cliff

## [1.0.0](https://github.com/Deanwfy/OverLingo/releases/tag/v1.0.0) - 2026-08-25

### Features

- Introduce OverLingo
