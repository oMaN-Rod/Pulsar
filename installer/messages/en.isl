; Pulsar's own installer text; Inno Setup's language file supplies the rest.
; Each language in pulsar.iss has a file like this one with the same keys.

[CustomMessages]
AutostartTask=Start Pulsar when I sign in
StartNow=Start Pulsar now
SensorsPageTitle=CPU temperature (optional)
SensorsPageSubtitle=Needs administrator rights and the PawnIO driver.
SensorsIntro=Pulsar runs without administrator rights, and Windows gives such programs no way to read the CPU temperature. To show it, Pulsar needs two extra pieces:%n%n  -  PawnIO, an open-source signed driver by namazso that reads the sensor.%n  -  The Pulsar sensor service, a small background service that asks PawnIO for the temperature and passes only that number to Pulsar.%n%nWindows will ask for administrator approval. Leave this off and Pulsar stays free of drivers and services; everything else works the same. Intel and AMD Ryzen CPUs. You can run this installer again to change your mind.
InstallSensors=Install the Pulsar sensor service
PawnIoInstalled=PawnIO is already installed
InstallPawnIo=Also install PawnIO (downloaded by winget)
PawnIoFailed=PawnIO could not be installed automatically. Its website will open so you can install it yourself; CPU temperature starts working once it is installed.
SensorsFailed=The sensor service was not installed, so CPU temperature will show a dash. Run this installer again to retry.
RemoveSettings=Also remove your Pulsar settings and logs?
