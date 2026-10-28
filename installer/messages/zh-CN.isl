; Simplified Chinese installer text for Pulsar; see en.isl.
; Based on the translation by JasonChow94 (https://github.com/JasonChow94/Pulsar).

[CustomMessages]
AutostartTask=登录 Windows 时启动 Pulsar
StartNow=立即启动 Pulsar
SensorsPageTitle=CPU 温度（可选）
SensorsPageSubtitle=需要管理员权限和 PawnIO 驱动。
SensorsIntro=Pulsar 无需管理员权限即可运行，但 Windows 不允许这类程序读取 CPU 温度。要显示 CPU 温度，Pulsar 还需要两个额外组件：%n%n  -  PawnIO：由 namazso 开发的开源签名驱动，用于读取传感器。%n  -  Pulsar 传感器服务：一个小型后台服务，向 PawnIO 读取温度，并只把这个数值传给 Pulsar。%n%nWindows 会请求管理员授权。不勾选时，Pulsar 不会安装任何驱动或服务，其他功能完全不受影响。支持 Intel 和 AMD Ryzen CPU。之后可以重新运行本安装程序更改选择。
InstallSensors=安装 Pulsar 传感器服务
PawnIoInstalled=已安装 PawnIO
InstallPawnIo=同时安装 PawnIO（通过 winget 下载）
PawnIoFailed=无法自动安装 PawnIO。即将打开其官网，请手动安装；安装完成后即可显示 CPU 温度。
SensorsFailed=传感器服务未安装，CPU 温度将显示为短横线。请重新运行本安装程序重试。
RemoveSettings=是否同时删除 Pulsar 的设置和日志？
