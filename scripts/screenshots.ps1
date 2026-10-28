#Requires -Version 7
<#
.SYNOPSIS
Captures the overlay, popups, menus, Settings and the details window to target\screenshots\.
.DESCRIPTION
Runs the release build on the live desktop with a throwaway settings folder, so your own
settings are never read or changed. It moves the real mouse over the taskbar: leave the PC
alone until it finishes, in about ten minutes. Exit Pulsar before running it.
.PARAMETER SkipBuild
Use the existing target\release binaries.
.PARAMETER Light
Also capture on a light taskbar. Switches Windows to light mode for a minute, then back.
.PARAMETER Publish
Copy each capture whose name is already in docs\images over that file.
.PARAMETER Parts
Which captures to take: overlay, popups, menus, settings, details.
#>
param(
    [string[]]$Parts = @('overlay', 'popups', 'menus', 'settings', 'details'),
    [switch]$SkipBuild,
    [switch]$Light,
    [switch]$Publish,
    [int]$Warmup = 45
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$bin = Join-Path $root 'target\release'
$out = Join-Path $root 'target\screenshots'

$running = Get-Process pulsar, pulsar-settings, pulsar-details -ErrorAction SilentlyContinue
if ($running) {
    throw "Exit Pulsar, its Settings and its details window first ($($running.Path -join ', '))"
}
if (-not $SkipBuild) {
    cargo build --release -p pulsar-monitor
    if ($LASTEXITCODE) { throw 'cargo build failed' }
}

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class Native {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public delegate bool EnumProc(IntPtr h, IntPtr l);

    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, string l, uint flags, uint ms, out IntPtr result);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern uint RegisterWindowMessage(string s);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, int d, IntPtr e);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
    [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
    [DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr m, int i);
    [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr h, IntPtr m, uint i, out RECT r);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint i, StringBuilder s, int n, uint flags);

    public static List<IntPtr> Windows(string cls) {
        var found = new List<IntPtr>();
        EnumWindows((h, l) => {
            var name = new StringBuilder(64);
            GetClassName(h, name, 64);
            if (name.ToString() == cls && IsWindowVisible(h)) found.Add(h);
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string MenuText(IntPtr m, int i) {
        var s = new StringBuilder(256);
        GetMenuString(m, (uint)i, s, 256, 0x400);
        return s.ToString();
    }
}
'@
[void][Native]::SetProcessDpiAwarenessContext([IntPtr]-4)

$Kinds = 'cpu', 'cpu_temp', 'cpu_power', 'cpu_clock', 'ram', 'chipset_temp', 'gpu', 'gpu_temp',
    'disk', 'disk_temp', 'network', 'ping', 'battery'
$Showcase = 'cpu', 'cpu_temp', 'ram', 'gpu', 'disk', 'network'
$All = $Kinds | Where-Object { $_ -ne 'battery' }

# A config with the given items shown, in order; everything else at its default.
function Write-Config {
    param(
        [string[]]$Show = $Showcase,
        [string]$Mode = 'graph',
        [string]$Labels = 'full',
        [switch]$Icons,
        [switch]$Inline,
        [string]$Style = 'area',
        [hashtable]$Styles = @{},
        [string]$Color = 'item',
        [string[]]$TextOnly = @(),
        [string]$Detail = 'standard',
        [string]$Background = 'translucent'
    )
    $toml = @"
version = 1

[general]
autostart = false
update_check = false
sample_interval_ms = 1000
history_len = 60

[display]
mode = "$Mode"
graph_inline = $("$Inline".ToLower())
labels = "$Labels"
icons = $("$Icons".ToLower())
hide_in_fullscreen = false

[popup]
detail = "$Detail"
background = "$Background"
graph = true
processes = 5

"@
    $order = @($Show) + @($Kinds | Where-Object { $_ -notin $Show })
    foreach ($kind in $order) {
        $style = if ($Styles.ContainsKey($kind)) { $Styles[$kind] } else { $Style }
        $toml += @"
[[items]]
kind = "$kind"
enabled = $("$($kind -in $Show)".ToLower())
graph = $("$($kind -notin $TextOnly)".ToLower())
style = "$style"
color_mode = "$Color"


"@
    }
    $dir = Join-Path $env:APPDATA 'Pulsar'
    New-Item -ItemType Directory -Force $dir | Out-Null
    Set-Content -Path (Join-Path $dir 'config.toml') -Value $toml -Encoding utf8NoBOM
}

function Send-ConfigChanged {
    $host_ = [Native]::FindWindow('PulsarHost', [NullString]::Value)
    $msg = [Native]::RegisterWindowMessage('Pulsar.ConfigChanged')
    [void][Native]::PostMessage($host_, $msg, [IntPtr]::Zero, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 2500
}

function Get-Rect([IntPtr]$h) {
    $r = New-Object Native+RECT
    [void][Native]::GetWindowRect($h, [ref]$r)
    $r
}

function Save-Screen([int]$Left, [int]$Top, [int]$Right, [int]$Bottom, [string]$Name) {
    $bmp = New-Object System.Drawing.Bitmap ($Right - $Left), ($Bottom - $Top)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($Left, $Top, 0, 0, $bmp.Size)
    $bmp.Save((Join-Path $out "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# Captures a window even when it is covered; returns a hash of the image, or of
# $Region (left, top, width, height in window pixels) when given.
function Save-Window([IntPtr]$h, [string]$Name, [int[]]$Region) {
    $r = Get-Rect $h
    $bmp = New-Object System.Drawing.Bitmap ($r.Right - $r.Left), ($r.Bottom - $r.Top)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $dc = $g.GetHdc()
    [void][Native]::PrintWindow($h, $dc, 2)
    $g.ReleaseHdc($dc)
    $path = Join-Path $out "$Name.png"
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $hashed = $bmp
    if ($Region) {
        $area = New-Object System.Drawing.Rectangle $Region[0], $Region[1], $Region[2], $Region[3]
        $area.Intersect((New-Object System.Drawing.Rectangle 0, 0, $bmp.Width, $bmp.Height))
        $hashed = $bmp.Clone($area, $bmp.PixelFormat)
    }
    $stream = New-Object System.IO.MemoryStream
    $hashed.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    $hash = [Convert]::ToHexString([System.Security.Cryptography.SHA256]::HashData($stream.ToArray()))
    if ($Region) { $hashed.Dispose() }
    $g.Dispose(); $bmp.Dispose(); $stream.Dispose()
    $hash
}

function Find-Window([string]$Class, [string]$Title) {
    for ($i = 0; $i -lt 60; $i++) {
        $cls = if ($Class) { $Class } else { [NullString]::Value }
        $ttl = if ($Title) { $Title } else { [NullString]::Value }
        $h = [Native]::FindWindow($cls, $ttl)
        if ($h -ne [IntPtr]::Zero) { return $h }
        Start-Sleep -Milliseconds 250
    }
    throw "No window $Class $Title"
}

function Park-Mouse {
    $tb = Get-Rect (Find-Window 'Shell_TrayWnd')
    [void][Native]::SetCursorPos([int](($tb.Left + $tb.Right) / 2), [int]($tb.Top - 400))
    Start-Sleep -Milliseconds 600
}

function Save-Overlay([string]$Name) {
    Park-Mouse
    $o = Get-Rect (Find-Window 'PulsarOverlay')
    $tb = Get-Rect (Find-Window 'Shell_TrayWnd')
    $pad = 12
    Save-Screen ($o.Left - $pad) $tb.Top ($o.Right + $pad) $tb.Bottom $Name
    Write-Host "  $Name"
}

function Get-Popup {
    $h = [Native]::FindWindow('PulsarPopup', [NullString]::Value)
    if ($h -ne [IntPtr]::Zero -and [Native]::IsWindowVisible($h)) { $h }
}

# Moves across the overlay and returns each item's centre and its popup's
# centre, found where the popup moves to a new place.
function Find-Items {
    Park-Mouse
    $o = Get-Rect (Find-Window 'PulsarOverlay')
    $y = [int](($o.Top + $o.Bottom) / 2)
    $step = [int]([Native]::GetDpiForWindow((Find-Window 'PulsarOverlay')) / 96.0 * 6)
    $spans = @()
    $keys = @()
    $last = $null
    for ($x = $o.Left + 2; $x -lt $o.Right - 1; $x += $step) {
        [void][Native]::SetCursorPos($x, $y)
        Start-Sleep -Milliseconds 450
        $p = Get-Popup
        $key = if ($p) { $r = Get-Rect $p; [int](($r.Left + $r.Right) / 2) } else { $null }
        if ($null -eq $key) { $last = $null; continue }
        if ($key -ne $last) { $spans += , @($x, $x); $keys += $key } else { $spans[-1][1] = $x }
        $last = $key
    }
    Park-Mouse
    for ($i = 0; $i -lt $spans.Count; $i++) {
        , @([int](($spans[$i][0] + $spans[$i][1]) / 2), $y, $keys[$i])
    }
}

function Update-Items {
    $script:items = @(Find-Items)
    if ($items.Count -ne $All.Count) {
        Write-Warning "found $($items.Count) items for $($All.Count); names may be off"
    }
}

# Captures the popups of the items at $Indexes. Values changing width can
# shift the overlay, so a popup that is not where the sweep found it means
# finding the items again.
function Save-Popups([int[]]$Indexes, [string]$Suffix) {
    foreach ($i in $Indexes) {
        $name = "popup-$($All[$i] -replace '_', '-')$Suffix"
        for ($attempt = 0; $attempt -lt 3; $attempt++) {
            $item = $items[$i]
            Park-Mouse
            [void][Native]::SetCursorPos($item[0], $item[1])
            Start-Sleep -Milliseconds 3000
            $p = Get-Popup
            $r = if ($p) { Get-Rect $p }
            if ($r -and [Math]::Abs(($r.Left + $r.Right) / 2 - $item[2]) -le 3) {
                Save-Screen $r.Left $r.Top $r.Right $r.Bottom $name
                Write-Host "  $name"
                break
            }
            Update-Items
        }
        if ($attempt -eq 3) { Write-Warning "no popup for $name" }
    }
    Park-Mouse
}

function Get-Slug([string]$Text) {
    (($Text -split "`t")[0] -replace '&', '' -replace '[^A-Za-z0-9]+', '-').Trim('-').ToLower()
}

# Captures every open menu together, clear around them.
function Save-Menus([string]$Name) {
    $rects = [Native]::Windows('#32768') | ForEach-Object { Get-Rect $_ }
    $l = ($rects | Measure-Object Left -Minimum).Minimum
    $t = ($rects | Measure-Object Top -Minimum).Minimum
    $r = ($rects | Measure-Object Right -Maximum).Maximum
    $b = ($rects | Measure-Object Bottom -Maximum).Maximum
    $bmp = New-Object System.Drawing.Bitmap ($r - $l), ($b - $t), ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    foreach ($m in $rects) {
        $size = New-Object System.Drawing.Size ($m.Right - $m.Left), ($m.Bottom - $m.Top)
        $g.CopyFromScreen($m.Left, $m.Top, $m.Left - $l, $m.Top - $t, $size)
    }
    $bmp.Save((Join-Path $out "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Host "  $Name"
}

# Hovers each submenu of $Menu in turn, capturing it with its parents, to $Depth levels.
function Save-Submenus([IntPtr]$Menu, [string]$Prefix, [int]$Depth) {
    $count = [Native]::GetMenuItemCount($Menu)
    for ($i = 0; $i -lt $count; $i++) {
        $sub = [Native]::GetSubMenu($Menu, $i)
        if ($sub -eq [IntPtr]::Zero) { continue }
        $r = New-Object Native+RECT
        if (-not [Native]::GetMenuItemRect([IntPtr]::Zero, $Menu, $i, [ref]$r)) { continue }
        [void][Native]::SetCursorPos([int](($r.Left + $r.Right) / 2), [int](($r.Top + $r.Bottom) / 2))
        Start-Sleep -Milliseconds 900
        $name = "$Prefix-$(Get-Slug ([Native]::MenuText($Menu, $i)))"
        Save-Menus $name
        if ($Depth -gt 1) { Save-Submenus $sub $name ($Depth - 1) }
        [void][Native]::SetCursorPos([int](($r.Left + $r.Right) / 2), [int](($r.Top + $r.Bottom) / 2))
        Start-Sleep -Milliseconds 500
    }
}

function Close-Menus {
    for ($i = 0; $i -lt 4 -and [Native]::Windows('#32768').Count; $i++) {
        [Native]::keybd_event(0x1B, 0, 0, [IntPtr]::Zero)
        [Native]::keybd_event(0x1B, 0, 2, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 300
    }
}

# Right-clicks the overlay at $At and returns the menu that opens.
function Open-Menu([int[]]$At) {
    Park-Mouse
    [void][Native]::SetCursorPos($At[0], $At[1])
    Start-Sleep -Milliseconds 300
    [Native]::mouse_event(0x0008, 0, 0, 0, [IntPtr]::Zero)
    [Native]::mouse_event(0x0010, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 1000
    $window = [Native]::Windows('#32768') | Select-Object -First 1
    if (-not $window) { throw 'No menu opened' }
    [Native]::SendMessage($window, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)
}

# Opens the details window from the menu, so it gets Pulsar's history.
function Open-Details {
    $o = Get-Rect (Find-Window 'PulsarOverlay')
    $menu = Open-Menu @([int](($o.Left + $o.Right) / 2), [int](($o.Top + $o.Bottom) / 2))
    for ($i = 0; $i -lt [Native]::GetMenuItemCount($menu); $i++) {
        if ([Native]::MenuText($menu, $i) -notlike 'System details*') { continue }
        $r = New-Object Native+RECT
        [void][Native]::GetMenuItemRect([IntPtr]::Zero, $menu, $i, [ref]$r)
        [void][Native]::SetCursorPos([int](($r.Left + $r.Right) / 2), [int](($r.Top + $r.Bottom) / 2))
        Start-Sleep -Milliseconds 300
        [Native]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
        [Native]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
        Park-Mouse
        return
    }
    Close-Menus
    throw 'No System details entry in the menu'
}

function Save-Menu([int[]]$At) {
    $menu = Open-Menu $At
    Save-Menus 'menu'
    Save-Submenus $menu 'menu' 2
    Close-Menus
    Park-Mouse
}

# Posts a click at a point in logical client coordinates, without moving the mouse.
function Send-Click([IntPtr]$h, [double]$X, [double]$Y) {
    $scale = [Native]::GetDpiForWindow($h) / 96.0
    $lp = [IntPtr]((([int]($Y * $scale)) -shl 16) -bor ([int]($X * $scale) -band 0xFFFF))
    [void][Native]::PostMessage($h, 0x0200, [IntPtr]0, $lp)
    Start-Sleep -Milliseconds 100
    [void][Native]::PostMessage($h, 0x0201, [IntPtr]1, $lp)
    Start-Sleep -Milliseconds 60
    [void][Native]::PostMessage($h, 0x0202, [IntPtr]0, $lp)
    Start-Sleep -Milliseconds 900
}

function Send-Wheel([IntPtr]$h, [double]$X, [double]$Y, [int]$Notches) {
    $scale = [Native]::GetDpiForWindow($h) / 96.0
    $client = [IntPtr]((([int]($Y * $scale)) -shl 16) -bor ([int]($X * $scale) -band 0xFFFF))
    $pt = New-Object Native+POINT
    $pt.X = [int]($X * $scale); $pt.Y = [int]($Y * $scale)
    [void][Native]::ClientToScreen($h, [ref]$pt)
    $screen = [IntPtr](($pt.Y -shl 16) -bor ($pt.X -band 0xFFFF))
    [void][Native]::PostMessage($h, 0x0200, [IntPtr]0, $client)
    for ($k = 0; $k -lt [Math]::Abs($Notches); $k++) {
        $delta = if ($Notches -lt 0) { -120 } else { 120 }
        [void][Native]::PostMessage($h, 0x020A, [IntPtr](($delta -band 0xFFFF) -shl 16), $screen)
        Start-Sleep -Milliseconds 60
    }
    Start-Sleep -Milliseconds 900
}

function Set-LightTaskbar([int]$Value) {
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
    Set-ItemProperty $key SystemUsesLightTheme $Value
    $result = [IntPtr]::Zero
    [void][Native]::SendMessageTimeout([IntPtr]0xFFFF, 0x001A, [IntPtr]::Zero, 'ImmersiveColorSet', 2, 3000, [ref]$result)
    Start-Sleep -Seconds 3
}

$work = Join-Path ([IO.Path]::GetTempPath()) "pulsar-screenshots-$PID"
$saved = @{ APPDATA = $env:APPDATA; LOCALAPPDATA = $env:LOCALAPPDATA }
$theme = (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize').SystemUsesLightTheme
$started = @()
try {
    $env:APPDATA = Join-Path $work 'Roaming'
    $env:LOCALAPPDATA = Join-Path $work 'Local'
    New-Item -ItemType Directory -Force $out | Out-Null
    $prefixes = @{ overlay = 'overlay-'; popups = 'popup-'; menus = 'menu'; settings = 'settings-'; details = 'details-' }
    Get-ChildItem $out -Filter *.png |
        Where-Object { $name = $_.Name; $Parts | Where-Object { $name.StartsWith($prefixes[$_]) } } |
        Remove-Item

    if ($Light -or ($Parts | Where-Object { $_ -ne 'settings' })) {
        Write-Config
        $started += Start-Process (Join-Path $bin 'pulsar.exe') -PassThru
        [void](Find-Window 'PulsarOverlay')
        Write-Host "Filling the graphs for $Warmup s"
        Park-Mouse
        Start-Sleep -Seconds $Warmup
    }

    if ('overlay' -in $Parts) {
        Write-Host 'Overlay'
        Save-Overlay 'overlay-graph'
        $layouts = [ordered]@{
            'overlay-graph-icons'  = @{ Icons = $true }
            'overlay-graph-short'  = @{ Labels = 'short' }
            'overlay-graph-inline' = @{ Inline = $true; Icons = $true }
            'overlay-text'         = @{ Mode = 'text' }
            'overlay-text-short'   = @{ Mode = 'text'; Labels = 'short' }
            'overlay-text-icons'   = @{ Mode = 'text'; Labels = 'none'; Icons = $true }
            'overlay-style-line'   = @{ Style = 'line' }
            'overlay-style-bars'   = @{ Style = 'bars' }
            'overlay-style-meter'  = @{ Style = 'meter' }
            'overlay-style-mixed'  = @{ Styles = @{ cpu_temp = 'line'; ram = 'meter'; gpu = 'bars' }; TextOnly = 'disk' }
            'overlay-color-accent' = @{ Color = 'accent' }
            'overlay-color-load'   = @{ Color = 'load' }
            'overlay-text-load'    = @{ Mode = 'text'; Color = 'load' }
            'overlay-all'          = @{ Show = $All }
        }
        foreach ($name in $layouts.Keys) {
            $options = $layouts[$name]
            Write-Config @options
            Send-ConfigChanged
            Save-Overlay $name
        }
    }

    if ($Light -or ($Parts | Where-Object { $_ -in 'popups', 'menus' })) {
        Write-Config -Show $All
        Send-ConfigChanged
        Update-Items
    }

    if ('popups' -in $Parts) {
        Write-Host 'Popups'
        foreach ($detail in 'simple', 'standard', 'detailed') {
            Write-Config -Show $All -Detail $detail
            Send-ConfigChanged
            Save-Popups (0..($All.Count - 1)) "-$detail"
        }
        Write-Config -Show $All -Background solid
        Send-ConfigChanged
        Save-Popups 0, 1 '-solid'
    }

    if ('menus' -in $Parts) {
        Write-Host 'Menus'
        Save-Menu $items[0][0..1]
    }

    if ($Light) {
        Write-Host 'Light taskbar'
        Set-LightTaskbar 1
        Write-Config
        Send-ConfigChanged
        Save-Overlay 'overlay-light'
        Write-Config -Mode text
        Send-ConfigChanged
        Save-Overlay 'overlay-text-light'
        Write-Config -Show $All
        Send-ConfigChanged
        Save-Popups 0 '-light'
        Write-Config -Show $All -Background solid
        Send-ConfigChanged
        Save-Popups 0 '-light-solid'
        Set-LightTaskbar $theme
    }

    if ('settings' -in $Parts) {
        Write-Host 'Settings'
        Write-Config
        $started += Start-Process (Join-Path $bin 'pulsar-settings.exe') -PassThru
        $settings = Find-Window $null 'Pulsar Settings'
        Start-Sleep -Seconds 2
        $pages = 'general', 'display', 'items', 'about'
        for ($i = 0; $i -lt $pages.Count; $i++) {
            Send-Click $settings 115 (28 + 44 * $i)
            $previous = Save-Window $settings "settings-$($pages[$i])"
            Write-Host "  settings-$($pages[$i])"
            for ($n = 2; $n -lt 12; $n++) {
                Send-Wheel $settings 500 300 -4
                $hash = Save-Window $settings "settings-$($pages[$i])-$n"
                if ($hash -eq $previous) {
                    Remove-Item (Join-Path $out "settings-$($pages[$i])-$n.png")
                    break
                }
                Write-Host "  settings-$($pages[$i])-$n"
                $previous = $hash
            }
        }
    }

    if ('details' -in $Parts) {
        Write-Host 'Details'
        Open-Details
        $details = Find-Window $null 'Pulsar Details'
        Start-Sleep -Seconds 3
        $scale = [Native]::GetDpiForWindow($details) / 96.0
        # The title bar and page heading, which change only with the page.
        $heading = @([int](260 * $scale), 0, [int](500 * $scale), [int](110 * $scale))
        $previous = $null
        for ($i = 0; $i -lt 14; $i++) {
            Send-Click $details 124 (80 + 54 * $i)
            Start-Sleep -Seconds 1
            $hash = Save-Window $details "details-$i" $heading
            if ($hash -eq $previous) {
                Remove-Item (Join-Path $out "details-$i.png")
                break
            }
            Write-Host "  details-$i"
            $previous = $hash
        }
    }
} finally {
    $started | Where-Object { -not $_.HasExited } | Stop-Process -Force
    Get-Process pulsar-details -ErrorAction SilentlyContinue |
        Where-Object Path -eq (Join-Path $bin 'pulsar-details.exe') | Stop-Process -Force
    if ($Light) { Set-LightTaskbar $theme }
    $env:APPDATA = $saved.APPDATA
    $env:LOCALAPPDATA = $saved.LOCALAPPDATA
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

if ($Publish) {
    Get-ChildItem $out -Filter *.png | ForEach-Object {
        $target = Join-Path $root "docs\images\$($_.Name)"
        if (Test-Path $target) { Copy-Item $_.FullName $target }
    }
}
Write-Host "Saved to $out"
