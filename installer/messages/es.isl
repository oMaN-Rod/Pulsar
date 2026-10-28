; Spanish installer text for Pulsar; see en.isl.

[CustomMessages]
AutostartTask=Iniciar Pulsar al iniciar sesión
StartNow=Iniciar Pulsar ahora
SensorsPageTitle=Temperatura de la CPU (opcional)
SensorsPageSubtitle=Requiere permisos de administrador y el controlador PawnIO.
SensorsIntro=Pulsar funciona sin permisos de administrador, y Windows no ofrece a esos programas ninguna forma de leer la temperatura de la CPU. Para mostrarla, Pulsar necesita dos componentes adicionales:%n%n  -  PawnIO, un controlador firmado de código abierto creado por namazso que lee el sensor.%n  -  El servicio de sensores de Pulsar, un pequeño servicio en segundo plano que pide la temperatura a PawnIO y solo pasa ese número a Pulsar.%n%nWindows pedirá la aprobación de un administrador. Si lo dejas desactivado, Pulsar no instala controladores ni servicios y todo lo demás funciona igual. CPU Intel y AMD Ryzen. Puedes volver a ejecutar este instalador si cambias de opinión.
InstallSensors=Instalar el servicio de sensores de Pulsar
PawnIoInstalled=PawnIO ya está instalado
InstallPawnIo=Instalar también PawnIO (descargado con winget)
PawnIoFailed=No se pudo instalar PawnIO automáticamente. Se abrirá su sitio web para que lo instales tú; la temperatura de la CPU empezará a funcionar en cuanto esté instalado.
SensorsFailed=El servicio de sensores no se instaló, así que la temperatura de la CPU mostrará un guion. Vuelve a ejecutar este instalador para intentarlo de nuevo.
RemoveSettings=¿Eliminar también la configuración y los registros de Pulsar?
