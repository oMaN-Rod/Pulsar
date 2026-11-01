; Korean installer text for Pulsar; see en.isl.

[CustomMessages]
AutostartTask=로그인할 때 Pulsar 시작
StartNow=지금 Pulsar 시작
SensorsPageTitle=CPU 온도(선택 사항)
SensorsPageSubtitle=관리자 권한과 PawnIO 드라이버가 필요합니다.
SensorsIntro=Pulsar는 관리자 권한 없이 실행되며, Windows는 이런 프로그램이 CPU 온도를 읽을 방법을 제공하지 않습니다. CPU 온도를 표시하려면 다음 두 구성 요소가 추가로 필요합니다.%n%n  -  PawnIO: namazso가 만든 서명된 오픈 소스 드라이버로, 센서를 읽습니다.%n  -  Pulsar 센서 서비스: PawnIO에서 온도를 읽어 그 값만 Pulsar에 전달하는 작은 백그라운드 서비스입니다.%n%nWindows에서 관리자 승인을 요청합니다. 선택하지 않으면 Pulsar는 드라이버나 서비스를 설치하지 않으며 다른 기능은 모두 똑같이 작동합니다. Intel 및 AMD Ryzen CPU를 지원합니다. 나중에 이 설치 프로그램을 다시 실행해 선택을 바꿀 수 있습니다.
InstallSensors=Pulsar 센서 서비스 설치
PawnIoInstalled=PawnIO가 이미 설치되어 있습니다
InstallPawnIo=PawnIO도 설치(winget으로 다운로드)
PawnIoFailed=PawnIO를 자동으로 설치할 수 없습니다. 웹사이트가 열리면 직접 설치하세요. 설치하면 CPU 온도가 표시됩니다.
SensorsFailed=센서 서비스가 설치되지 않아 CPU 온도가 대시(-)로 표시됩니다. 다시 시도하려면 이 설치 프로그램을 다시 실행하세요.
RemoveSettings=Pulsar 설정과 로그도 제거하시겠습니까?
