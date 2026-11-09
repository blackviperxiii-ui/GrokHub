### Fixed

- In the desktop app on Windows, a running game now frees the GPU within one sample: the app's local AI sampler reads the game guard every sample, as the other shell already did. The app's sampler now runs the shared governor step, so both shells pause, throttle, restart and scale the same way.
- Switching local models no longer stalls the window while the old engine stops: it stops off the UI thread.
- Changing the context size in local AI tuning no longer overrides the context a running server really launched with; the relaunch sets it.
