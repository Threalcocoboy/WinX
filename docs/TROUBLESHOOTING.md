# Troubleshooting WinX

## Connected for audio, unavailable in WinX

Pair the device in Windows Bluetooth settings. Close other companion apps that may hold its control connection, then open **Settings → Devices & Bluetooth → Refresh devices**. Try selecting the correct paired device again. If your model connects with limited controls, report the model, firmware, and which controls work. [Compatibility](../README.md#device-compatibility)

## A control or media overlay is missing

EQ, noise control, bass, and other features depend on the model and firmware. WinX hides unavailable controls. A media overlay requires a verified device link, a source-identified hardware command, and a matching Windows playback or volume change. Some AVRCP actions reach Windows without a source WinX can identify. Check **Settings → Notifications & Overlays** for the relevant switch and try the built-in overlay preview.

## Talk Mode did not fully restore

Reconnect the original headphones or earbuds and playback endpoint. Use **Settings → Talk Mode → Retry Recovery**. If you changed the volume or listening settings yourself during Talk Mode, WinX can preserve your newer values. Saved records are retained for targets WinX cannot reach. [Recovery details](WINX-TALK-MODE.md)

## Update checks or downloads fail

Check your connection and open the official GitHub release from **Settings → Updates**. Respect GitHub's cooldown if you were rate limited. If a download was interrupted, free disk space and choose **Update Now** again. If verification fails, discard the download and use a fresh copy. If Setup fails, WinX reports the failure and attempts to reopen the previous version. [Update guide](UPDATING.md)

## Windows shows an unknown publisher

WinX releases currently use an unsigned installer. Verify the SHA-256 hash against the official release before deciding whether to run it. Keep Windows security enabled. If your organization's policies block unsigned Setup, consult your administrator.

## WinX cannot open WebView2

Install or repair Microsoft's [Evergreen WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/), then start WinX again. For the portable build, extract all files together so the loader stays beside the executable.
