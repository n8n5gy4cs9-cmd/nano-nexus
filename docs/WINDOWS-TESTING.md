# Windows testing notes

## LAN Share and the Windows firewall
The first time sharing starts, Windows Defender Firewall asks whether to allow NanoNexus on **Private networks**
or **Public networks**. Tick **Private networks only**. If the prompt was dismissed:

1. Windows Security > Firewall & network protection > Allow an app through firewall.
2. Find NanoNexus and tick **Private**; leave **Public** unticked.
3. From a phone on the same Wi-Fi open `http://<pc-ip>:9889`. `ipconfig` shows the IPv4 address.

Checks: `netstat -ano | findstr 9889` shows the app listening, and turning sharing off closes the port.
Only the share port is reachable from the network; `llama-server` stays on 127.0.0.1.
