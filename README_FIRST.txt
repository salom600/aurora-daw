════════════════════════════════════════════════════════════════
  AURORA PRODUCER SUITE — READ THIS FIRST (Windows)
════════════════════════════════════════════════════════════════

1) HOW TO START
   • Unzip the WHOLE folder somewhere local (not inside the zip viewer).
   • Double-click START_AURORA.bat  (or aurora-daw-windows.exe directly).
   • If Windows SmartScreen appears ("Windows protected your PC"):
       click  More info  →  Run anyway.
     This appears because the app is free and not code-signed.
     The build is produced by GitHub Actions from the public repository;
     the same exe passed a 14-test engine self-test during packaging.

2) PROVE IT IS REAL — 30-SECOND TOUR
   • Press  PLAY (or Space): you HEAR the demo song through your sound card.
   • Top-right shows your real audio devices:
       OUT = your speakers/headphones driver
       IN  = your microphone  (the green meter moves when it hears you)
   • RECORD YOUR VOICE:
       click  O  on the vocal track header  → live monitoring (headphones!)
       click  R  to arm the track
       press the red  REC  button in the transport → sing → press again.
       Your take appears as a clip on the timeline.
   • ONE-CLICK AI VOCAL CLEANUP:
       select the take, open the AI tools panel (left) → "Clean Vocals".
       Removes noise, clicks, breaths, hum and harshness. Before/after
       report shows measured noise reduction.
   • EDIT: drag clips, S to split, Ctrl+D duplicate, Del to delete,
       double-click MIDI clips to open the piano roll.
   • MIX: faders, pan, solo/mute, FX rack (EQ, compressor, reverb, delay…).
   • EXPORT: File → Export (Ctrl+E) → WAV 16/24/32-bit or MP3, or stems.

3) IF SOMETHING GOES WRONG
   • No sound? Check OUT device in the top-right; Windows mixer volume.
   • Window does not open? The app automatically tries OpenGL, then
     DirectX/Vulkan. Update your GPU driver. Log file:
       %LOCALAPPDATA%\AuroraDAW\aurora.log
   • Headless sanity check: run in a terminal
       aurora-daw-windows.exe --selftest
     → runs the full engine test suite and prints PASS/FAIL.

4) PROJECT FILES
   Saved sessions live in  %USERPROFILE%\Music\Aurora  (*.aurora).

─── This is real software: Rust audio engine + WASAPI + AI DSP. ───
