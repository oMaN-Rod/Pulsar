<#
Records what Windows reports about the taskbar, as JSON, for use as test data.
Read-only: it only reads window and UI Automation properties and the registry.
It clicks nothing, moves nothing, and does not need Pulsar running.

App names and window titles are replaced with placeholders, because they can
hold document names. Run it while the taskbar is in the state to record.

    pwsh -File scripts\record-taskbar.ps1 -Label overflow-centred -Samples 5

See docs\taskbar-probe.md.
#>
param(
    [Parameter(Mandatory)][string]$Label,
    [string]$OutDir = (Join-Path $PSScriptRoot '..\target\taskbar-fixtures'),
    [int]$Samples = 3,
    [int]$IntervalSeconds = 2,
    [int]$MaxDepth = 16
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -Namespace TaskbarProbe -Name Native -MemberDefinition @'
public delegate bool EnumProc(IntPtr h, IntPtr l);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint c);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
[DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr h, int i);
[StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
[DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr h, uint f);
[StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct MI { public int Size; public R Monitor; public R Work; public uint Flags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string Device; }
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool GetMonitorInfoW(IntPtr m, ref MI i);
'@
$N = [TaskbarProbe.Native]
$NullText = [NullString]::Value

function Rect($r) { @{ left = $r.L; top = $r.T; right = $r.Rt; bottom = $r.B } }

function ClassOf([IntPtr]$h) {
    $s = New-Object Text.StringBuilder 128
    [void]$N::GetClassNameW($h, $s, 128)
    $s.ToString()
}

function WindowTree([IntPtr]$parent) {
    $kids = @()
    $c = $N::FindWindowExW($parent, [IntPtr]::Zero, $NullText, $NullText)
    while ($c -ne [IntPtr]::Zero) {
        $r = New-Object TaskbarProbe.Native+R
        [void]$N::GetWindowRect($c, [ref]$r)
        $kids += @{
            class    = ClassOf $c
            rect     = Rect $r
            visible  = $N::IsWindowVisible($c)
            children = @(WindowTree $c)
        }
        $c = $N::GetWindow($c, 2)
    }
    $kids
}

# Only shell-owned labels are kept; everything else may name a document.
function SafeName($current) {
    $id = "$($current.AutomationId)"
    $class = "$($current.ClassName)"
    if ($id -like 'Appid:*' -or $class -like 'Taskbar.TaskListButton*') { return '<app>' }
    if ($class -like 'SystemTray.*') { return '<tray>' }
    $name = "$($current.Name)"
    if ($name.Length -gt 40) { return '<long>' }
    $name
}

function SafeId($id) {
    if ("$id" -like 'Appid:*') { 'Appid:<app>' } else { "$id" }
}

function UiaTree($element, $walker, [int]$depth) {
    $c = $element.Current
    $b = $c.BoundingRectangle
    $empty = $b.IsEmpty -or [double]::IsInfinity($b.Width)
    $node = [ordered]@{
        class     = "$($c.ClassName)"
        id        = SafeId $c.AutomationId
        type      = $c.ControlType.ProgrammaticName -replace '^ControlType\.', ''
        offscreen = $c.IsOffscreen
        rect      = if ($empty) { $null } else { @{ left = [int]$b.Left; top = [int]$b.Top; right = [int]$b.Right; bottom = [int]$b.Bottom } }
        name      = SafeName $c
        children  = @()
    }
    if ($depth -lt $MaxDepth) {
        $kid = $walker.GetFirstChild($element)
        while ($kid) {
            $node.children += UiaTree $kid $walker ($depth + 1)
            $kid = $walker.GetNextSibling($kid)
        }
    }
    $node
}

function Taskbars {
    $found = New-Object Collections.ArrayList
    $callback = [TaskbarProbe.Native+EnumProc] {
        param($h, $l)
        $class = ClassOf $h
        if ($class -eq 'Shell_TrayWnd' -or $class -eq 'Shell_SecondaryTrayWnd') { [void]$found.Add($h) }
        $true
    }
    [void]$N::EnumWindows($callback, [IntPtr]::Zero)
    $found
}

function Bar([IntPtr]$h) {
    $r = New-Object TaskbarProbe.Native+R
    [void]$N::GetWindowRect($h, [ref]$r)
    $mi = New-Object TaskbarProbe.Native+MI
    $mi.Size = [Runtime.InteropServices.Marshal]::SizeOf($mi)
    [void]$N::GetMonitorInfoW($N::MonitorFromWindow($h, 2), [ref]$mi)
    $uia = $null
    $failure = $null
    try {
        $element = [Windows.Automation.AutomationElement]::FromHandle($h)
        $uia = UiaTree $element ([Windows.Automation.TreeWalker]::RawViewWalker) 0
    } catch {
        $failure = "$_"
    }
    [ordered]@{
        class    = ClassOf $h
        rect     = Rect $r
        dpi      = $N::GetDpiForWindow($h)
        rtl      = ($N::GetWindowLongW($h, -20) -band 0x00400000) -ne 0
        monitor  = Rect $mi.Monitor
        work     = Rect $mi.Work
        primary  = ($mi.Flags -band 1) -ne 0
        windows  = @(WindowTree $h)
        uia      = $uia
        uia_error = $failure
    }
}

function Settings {
    $advanced = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -ErrorAction SilentlyContinue
    $os = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    [ordered]@{
        build                = "$($os.CurrentBuild).$($os.UBR)"
        display_version      = "$($os.DisplayVersion)"
        TaskbarAl            = $advanced.TaskbarAl
        TaskbarGlomLevel     = $advanced.TaskbarGlomLevel
        MMTaskbarGlomLevel   = $advanced.MMTaskbarGlomLevel
        TaskbarSmallIcons    = $advanced.TaskbarSmallIcons
        TaskbarSd            = $advanced.TaskbarSd
        TaskbarDa            = $advanced.TaskbarDa
        MMTaskbarEnabled     = $advanced.MMTaskbarEnabled
        ShellExperienceHost  = @(Get-Process StartMenuExperienceHost -ErrorAction SilentlyContinue).Count
        pulsar_running       = [bool](Get-Process pulsar -ErrorAction SilentlyContinue)
    }
}

$recorded = @()
for ($i = 0; $i -lt $Samples; $i++) {
    if ($i -gt 0) { Start-Sleep -Seconds $IntervalSeconds }
    $recorded += [ordered]@{
        taken    = (Get-Date).ToString('o')
        taskbars = @(Taskbars | ForEach-Object { Bar $_ })
    }
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
$path = Join-Path $OutDir ("{0}-{1}.json" -f $Label, (Get-Date -Format 'yyyyMMdd-HHmmss'))
[ordered]@{
    schema   = 1
    label    = $Label
    settings = Settings
    samples  = $recorded
} | ConvertTo-Json -Depth 64 | Set-Content -Encoding utf8 $path
$size = (Get-Item $path).Length
"saved $path ($size bytes), $($recorded.Count) sample(s), $($recorded[0].taskbars.Count) taskbar(s)"
