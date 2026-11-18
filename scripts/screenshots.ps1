#Requires -Version 7
<#
.SYNOPSIS
Captures the overlay, popups, menus, the desktop widget, the game strip, Settings and the
details window to target\screenshots\.
.DESCRIPTION
Runs the release build on the live desktop with a throwaway settings folder, so your own
settings are never read or changed. It moves the real mouse over the taskbar and opens
Character Map for the game strip: leave the PC alone until it finishes, in about fifteen minutes for
everything, less with -Parts or -Only. Exit Pulsar before running it. The frame rate item and temperature dots need the sensor service, and
the battery cell needs a battery.
.PARAMETER SkipBuild
Use the existing target\release binaries.
.PARAMETER Light
Also capture on a light taskbar. Switches Windows to light mode for a minute, then back.
.PARAMETER Publish
Copy each capture the README uses that docs\images does not have yet. Live readings change every
capture, so existing images stay until named in -Replace.
.PARAMETER Replace
With -Publish, the captures to copy over existing images because their UI changed. Wildcards work.
.PARAMETER Parts
Which groups of captures to take: overlay, popups, menus, widget, game, settings, details, and
motion: an animated GIF of the overlay going through its styles, which needs ffmpeg and is taken only
when named. With -Only and no -Parts, the groups those captures belong to.
.PARAMETER Only
The captures to take, by name or wildcard, such as popup-cpu-liquid, widget-layout-* or
settings-popups; -List prints the names. A Settings page brings its scrolled views, such as
settings-popups-2. Menus and the details window are taken whole, then trimmed to what was asked.
Captures from earlier runs that are not asked for stay in target\screenshots, so -Publish still sees them.
.PARAMETER List
Print the name of every capture, by group, and stop; with -Only or -Parts, just those it would take.
.PARAMETER Ffmpeg
The ffmpeg executable for the motion capture.
#>
param(
    [string[]]$Parts = @('overlay', 'popups', 'menus', 'widget', 'game', 'settings', 'details'),
    [string[]]$Only = @(),
    [switch]$List,
    [switch]$SkipBuild,
    [switch]$Light,
    [switch]$Publish,
    [string[]]$Replace = @(),
    [int]$Warmup = 45,
    [string]$Ffmpeg = 'ffmpeg'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$bin = Join-Path $root 'target\release'
$out = Join-Path $root 'target\screenshots'



$Kinds = 'cpu', 'cpu_temp', 'cpu_power', 'cpu_clock', 'ram', 'chipset_temp', 'gpu', 'gpu_temp',
    'gpu_memory', 'disk', 'disk_temp', 'network', 'ping', 'battery', 'fps'
$Showcase = 'cpu', 'cpu_temp', 'ram', 'gpu', 'disk', 'network'
# Network has no load scale, so it cannot tint or show a status bubble.
$Loaded = 'cpu', 'cpu_temp', 'ram', 'gpu', 'gpu_memory', 'gpu_temp'
$All = $Kinds | Where-Object { $_ -ne 'battery' }

$OverlayShots = [ordered]@{
    'overlay-graph-icons'  = @{ Icons = $true }
    'overlay-graph-short'  = @{ Labels = 'short' }
    'overlay-graph-inline' = @{ Inline = $true; Icons = $true }
    'overlay-vertical-labels' = @{ VerticalLabels = $true }
    'overlay-text'         = @{ Mode = 'text' }
    'overlay-text-short'   = @{ Mode = 'text'; Labels = 'short' }
    'overlay-text-icons'   = @{ Mode = 'text'; Labels = 'none'; Icons = $true }
    'overlay-text-label-above' = @{ Mode = 'text'; LabelAbove = $Showcase }
    'overlay-shape-pill'   = @{ Shape = 'pill' }
    'overlay-shape-chips'  = @{ Shape = 'chips' }
    'overlay-style-line'   = @{ Style = 'line' }
    'overlay-style-bars'   = @{ Style = 'bars' }
    'overlay-style-meter'  = @{ Style = 'meter' }
    'overlay-style-segments' = @{ Style = 'segments' }
    'overlay-style-spark'  = @{ Style = 'spark' }
    'overlay-style-ring'   = @{ Style = 'ring' }
    'overlay-style-dot-matrix' = @{ Style = 'dot_matrix' }
    'overlay-style-heat-strip' = @{ Style = 'heat_strip' }
    'overlay-style-bullet' = @{ Style = 'bullet' }
    'overlay-style-range-rail' = @{ Style = 'range_rail' }
    'overlay-style-tint'   = @{ Show = $Loaded; Style = 'tint' }
    'overlay-style-bubble' = @{ Show = $Loaded; Style = 'bubble' }
    'overlay-style-combined' = @{ Show = 'cpu', 'gpu'; Style = 'combined' }
    'overlay-style-cores'  = @{ Show = 'cpu', 'ram'; Styles = @{ cpu = 'cores' } }
    'overlay-style-heat'   = @{ Show = 'cpu', 'ram'; Styles = @{ cpu = 'heat' } }
    'overlay-style-core-types' = @{ Show = 'cpu', 'ram'; Styles = @{ cpu = 'core_types' } }
    'overlay-style-mirror' = @{ Show = 'network', 'ram'; Styles = @{ network = 'mirror_bars' } }
    'overlay-style-uptime' = @{ Show = 'ping', 'ram'; Styles = @{ ping = 'uptime' } }
    'overlay-style-mixed'  = @{ Styles = @{ cpu_temp = 'line'; ram = 'meter'; gpu = 'bars' }; TextOnly = 'disk' }
    'overlay-trend'        = @{ Trend = $Showcase }
    'overlay-color-accent' = @{ Color = 'accent' }
    'overlay-color-load'   = @{ Color = 'load' }
    'overlay-color-custom' = @{ Color = 'load'; LoadColors = @{ low = '#4CC2FF'; moderate = '#B18CFF'; high = '#FF8CC6'; critical = '#FF4D6A' } }
    'overlay-text-load'    = @{ Mode = 'text'; Color = 'load' }
    'overlay-all'          = @{ Show = $All }
}
if (Get-CimInstance Win32_Battery) {
    $OverlayShots['overlay-style-battery'] = @{ Show = 'battery', 'ram'; Styles = @{ battery = 'battery_cell' } }
}

$WidgetShots = [ordered]@{
    'widget-sparklines' = @{ Widget = 'sparklines' }
    'widget-tiles'      = @{ Widget = 'tiles' }
    'widget-meters'     = @{ Widget = 'meters' }
    'widget-dials'      = @{ Widget = 'dials' }
    'widget-core-wall'  = @{ Widget = 'sparklines'; WidgetStyles = @{ cpu = 'core_wall' } }
    'widget-detailed'   = @{ Widget = 'sparklines'; WidgetDetail = 'detailed' }
    'widget-background' = @{ Widget = 'sparklines'; WidgetBackground = '#1E2A44' }
    'widget-cards'      = @{ Widget = 'cards'; WidgetSeparate = $true }
    'widget-glass'      = @{ Widget = 'sparklines'; WidgetMaterial = 'glass'; WidgetSeparate = $true }
    'widget-liquid'     = @{ Widget = 'sparklines'; WidgetMaterial = 'liquid'; WidgetSeparate = $true }
    'widget-acrylic'    = @{ Widget = 'sparklines'; WidgetMaterial = 'acrylic' }
    'widget-layout-dashboard' = @{ Widget = 'sparklines'; WidgetLayout = 'dashboard' }
    'widget-layout-strip'  = @{ Widget = 'sparklines'; WidgetLayout = 'strip' }
    'widget-layout-rings'  = @{ Widget = 'sparklines'; WidgetLayout = 'rings' }
    'widget-layout-focus'  = @{ Widget = 'sparklines'; WidgetLayout = 'focus'; WidgetItems = 'cpu' }
}

# Navigation rows and in-page tabs, by their centre in logical client pixels.
$SettingsPages = [ordered]@{
    items = 97; layout = 137; theme = 177; popups = 217; widget = 257; sensors = 331
    devices = 371; profiles = 445; shortcuts = 485; general = 525; about = 565
}
$SettingsTabY = 97
$SettingsTabs = @{
    items   = [ordered]@{ '' = 287; 'load-colors' = 370 }
    layout  = [ordered]@{ '' = 294; 'text' = 359; 'position' = 424 }
    widget  = [ordered]@{ '' = 302; 'look' = 378; 'items' = 438 }
    devices = [ordered]@{ '' = 283; 'graphics' = 351; 'network' = 434; 'ping' = 503 }
}

$Details = 'simple', 'standard', 'detailed'
$Materials = 'solid', 'glass', 'liquid', 'clear'
$Groups = [ordered]@{
    overlay = 'overlay-'; popups = 'popup-'; menus = 'menu'; widget = 'widget-'; game = 'game-'
    settings = 'settings-'; details = 'details-'; motion = 'overlay-motion'
}

function Get-PopupName([int]$Index, [string]$Suffix) { "popup-$($All[$Index] -replace '_', '-')$Suffix" }

function Get-SettingsViews {
    foreach ($page in $SettingsPages.Keys) {
        $views = if ($SettingsTabs.ContainsKey($page)) { $SettingsTabs[$page].Keys } else { @('') }
        foreach ($tab in $views) { if ($tab) { "settings-$page-$tab" } else { "settings-$page" } }
    }
}

# Whether -Only asks for a capture, by its name or by one of its scrolled views.
function Test-Wanted([string]$Name) {
    -not $Only -or [bool]($Only | Where-Object { $Name -like $_ -or $_ -like "$Name-[0-9]*" })
}

# Whether a saved file is one -Only asked for, or a scrolled view of one.
function Test-WantedFile([string]$Name) {
    [bool]($Only | Where-Object { $Name -like $_ -or $Name -like "$_-[0-9]*" })
}

# Every capture by group. Menus and the details window name theirs as they go.
$Catalog = [ordered]@{
    overlay  = @('overlay-graph') + @($OverlayShots.Keys) +
        'overlay-floating', 'overlay-floating-pill', 'overlay-light', 'overlay-text-light'
    popups   = @(foreach ($d in $Details) { 0..($All.Count - 1) | ForEach-Object { Get-PopupName $_ "-$d" } }) +
        @(foreach ($m in $Materials) { 0, 1 | ForEach-Object { Get-PopupName $_ "-$m" } }) +
        (Get-PopupName 0 '-light'), (Get-PopupName 0 '-light-solid')
    menus    = @()
    widget   = @($WidgetShots.Keys)
    game     = @('game-strip')
    settings = @(Get-SettingsViews)
    details  = @()
    motion   = @('overlay-motion')
}
$LightOnly = 'overlay-light', 'overlay-text-light', (Get-PopupName 0 '-light'), (Get-PopupName 0 '-light-solid')
$Notes = @{
    menus    = 'menu-*, named after the menu items'
    settings = 'and -2, -3 ... for views that scroll'
    details  = 'details-0 ... one per page'
}

if ($Only -and -not $PSBoundParameters.ContainsKey('Parts')) {
    $Parts = @(foreach ($group in $Catalog.Keys) {
        $prefix = $Groups[$group]
        $named = $Catalog[$group] | Where-Object { Test-Wanted $_ }
        $unnamed = $Catalog[$group].Count -eq 0 -and ($Only | Where-Object {
            $literal = ($_ -split '[*?\[]')[0]
            $_ -eq '*' -or ($literal -and ($literal.StartsWith($prefix) -or $prefix.StartsWith($literal)))
        })
        if ($named -or $unnamed) { $group }
    })
    if (-not $Parts) { throw "No capture is named like $($Only -join ', '); -List prints the names" }
    Write-Host "Groups: $($Parts -join ', ')"
}

if ($List) {
    $everything = -not $Only -and -not $PSBoundParameters.ContainsKey('Parts')
    foreach ($group in $Catalog.Keys) {
        if ($group -notin $Parts -and -not ($everything -and $group -eq 'motion')) { continue }
        $names = @($Catalog[$group] | Where-Object { Test-Wanted $_ } | ForEach-Object {
            if ($_ -in $LightOnly) { "$_ (-Light)" } elseif ($group -eq 'motion') { "$_.gif" } else { $_ }
        })
        if ($Notes.ContainsKey($group)) { $names += $Notes[$group] }
        Write-Host $group -ForegroundColor Cyan
        $names | ForEach-Object { Write-Host "  $_" }
    }
    return
}

$running = Get-Process pulsar, pulsar-settings, pulsar-details -ErrorAction SilentlyContinue
if ($running) {
    throw "Exit Pulsar, its Settings and its details window first ($($running.Path -join ', '))"
}
if (-not $SkipBuild) {
    cargo build --release -p pulsar-monitor
    if ($LASTEXITCODE) { throw 'cargo build failed' }
}

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
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

# A config with the given items shown, in order; everything else at its default.
function Write-Config {
    param(
        [string[]]$Show = $Showcase,
        [string]$Mode = 'graph',
        [string]$Labels = 'full',
        [switch]$Icons,
        [switch]$Inline,
        [switch]$VerticalLabels,
        [string]$Style = 'area',
        [hashtable]$Styles = @{},
        [string[]]$Trend = @(),
        [string]$Color = 'item',
        # Band names to #RRGGBB, for color by load.
        [hashtable]$LoadColors = @{},
        [string[]]$TextOnly = @(),
        [string[]]$LabelAbove = @(),
        [string]$Shape = 'rectangle',
        [string]$Detail = 'standard',
        [string]$Background = 'acrylic',
        # Floating above the taskbar with a panel behind it.
        [switch]$Floating,
        # A widget style turns the widget on, drawn above other windows.
        [string]$Widget,
        [string]$WidgetDetail = 'simple',
        [hashtable]$WidgetStyles = @{},
        [string]$WidgetBackground,
        [string]$WidgetMaterial = 'solid',
        [switch]$WidgetSeparate,
        [int]$WidgetGlassTint = 0,
        [switch]$WidgetBorder,
        [string]$WidgetLayout = 'auto',
        [string[]]$WidgetItems = @(),
        # Executable names to count as games, with the strip on.
        [string[]]$Games = @()
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
vertical_labels = $("$VerticalLabels".ToLower())
labels = "$Labels"
icons = $("$Icons".ToLower())
hide_in_fullscreen = false
shape = "$Shape"
$(if ($Floating) { "position = `"floating`"`npanel_opacity = 75" })

[display.load_colors]
$(($LoadColors.GetEnumerator() | ForEach-Object { "$($_.Key) = `"$($_.Value)`"" }) -join "`n")

[popup]
detail = "$Detail"
background = "$Background"
graph = true
processes = 5

"@
    if ($Widget) {
        $toml += @"
[widget]
enabled = true
layer = "on_top"
style = "$Widget"
detail = "$WidgetDetail"
opacity = 100
hide_in_fullscreen = false
material = "$WidgetMaterial"
separate = $("$WidgetSeparate".ToLower())
glass_tint = $WidgetGlassTint
border = $("$WidgetBorder".ToLower())
layout = "$WidgetLayout"
items = [$(($WidgetItems | ForEach-Object { "`"$_`"" }) -join ', ')]
$(if ($WidgetBackground) { "background = `"$WidgetBackground`"" })

"@
    }
    if ($Games) {
        $toml += @"
[game]
strip = true
games = [$(($Games | ForEach-Object { "`"$_`"" }) -join ', ')]

"@
    }
    $order = @($Show) + @($Kinds | Where-Object { $_ -notin $Show })
    foreach ($kind in $order) {
        $style = if ($Styles.ContainsKey($kind)) { $Styles[$kind] } else { $Style }
        $toml += @"
[[items]]
kind = "$kind"
enabled = $("$($kind -in $Show)".ToLower())
graph = $("$($kind -notin $TextOnly)".ToLower())
style = "$style"
trend = $("$($kind -in $Trend)".ToLower())
label_above = $("$($kind -in $LabelAbove)".ToLower())
color_mode = "$Color"
$(if ($WidgetStyles.ContainsKey($kind)) { "widget_style = `"$($WidgetStyles[$kind])`"" })


"@
    }
    $dir = Join-Path $env:APPDATA 'Pulsar'
    New-Item -ItemType Directory -Force $dir | Out-Null
    Set-Content -Path (Join-Path $dir 'config.toml') -Value $toml -Encoding utf8NoBOM
}

# Two profiles for the menu and Settings: one matching the current config, one not.
function Write-Profiles {
    $dir = Join-Path $env:APPDATA 'Pulsar\profiles'
    New-Item -ItemType Directory -Force $dir | Out-Null
    $config = Get-Content (Join-Path $env:APPDATA 'Pulsar\config.toml') -Raw
    Set-Content (Join-Path $dir 'desk.toml') "name = `"Desk`"`n$config" -Encoding utf8NoBOM
    Set-Content (Join-Path $dir 'gaming.toml') "name = `"Gaming`"`n[display]`nmode = `"text`"`n" -Encoding utf8NoBOM
}

# Captures a window from the screen, with $Pad pixels around it.
function Save-Around([IntPtr]$h, [string]$Name, [int]$Pad = 12) {
    $r = Get-Rect $h
    Save-Screen ($r.Left - $Pad) ($r.Top - $Pad) ($r.Right + $Pad) ($r.Bottom + $Pad) $Name
    Write-Host "  $Name"
}

# A gradient window under $Rect, for a blurring material to show against.
function Show-Backdrop($Rect, [int]$Pad = 80) {
    $form = New-Object System.Windows.Forms.Form
    $form.FormBorderStyle = 'None'
    $form.ShowInTaskbar = $false
    $form.StartPosition = 'Manual'
    $form.Bounds = New-Object System.Drawing.Rectangle ($Rect.Left - $Pad), ($Rect.Top - $Pad), ($Rect.Right - $Rect.Left + 2 * $Pad), ($Rect.Bottom - $Rect.Top + 2 * $Pad)
    $bmp = New-Object System.Drawing.Bitmap $form.Width, $form.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $area = New-Object System.Drawing.Rectangle 0, 0, $bmp.Width, $bmp.Height
    $brush = New-Object System.Drawing.Drawing2D.LinearGradientBrush $area, ([System.Drawing.Color]::Black), ([System.Drawing.Color]::Black), 35.0
    $blend = New-Object System.Drawing.Drawing2D.ColorBlend 3
    $blend.Colors = [System.Drawing.ColorTranslator]::FromHtml('#22CDE9'), [System.Drawing.ColorTranslator]::FromHtml('#3F35BD'), [System.Drawing.ColorTranslator]::FromHtml('#121934')
    $blend.Positions = 0.0, 0.5, 1.0
    $brush.InterpolationColors = $blend
    $g.FillRectangle($brush, $area)
    $brush.Dispose(); $g.Dispose()
    $form.BackgroundImage = $bmp
    $form.Show()
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 1500
    [System.Windows.Forms.Application]::DoEvents()
    $form
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
        # On the taskbar the overlay is the taskbar's child, which FindWindow skips.
        if ($Class -eq 'PulsarOverlay') {
            $bar = [Native]::FindWindow('Shell_TrayWnd', [NullString]::Value)
            $h = [Native]::FindWindowEx($bar, [IntPtr]::Zero, $Class, [NullString]::Value)
            if ($h -ne [IntPtr]::Zero) { return $h }
        }
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
        $name = Get-PopupName $i $Suffix
        if (-not (Test-Wanted $name)) { continue }
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
    $runStarted = Get-Date
    Get-ChildItem $out -Filter *.png |
        Where-Object {
            $name = $_.BaseName
            $grouped = $Parts | Where-Object { $name.StartsWith($Groups[$_]) }
            $grouped -and (-not $Only -or (Test-WantedFile $name))
        } |
        Remove-Item
    # Menus and the details window save every view they pass, so earlier
    # captures they overwrite are put back once the run is trimmed.
    $kept = Join-Path $work 'kept'
    if ($Only) {
        New-Item -ItemType Directory -Force $kept | Out-Null
        Get-ChildItem $out -Filter *.png | Copy-Item -Destination $kept
    }

    Write-Config
    Write-Profiles
    if ($Light -or ($Parts | Where-Object { $_ -ne 'settings' })) {
        $started += Start-Process (Join-Path $bin 'pulsar.exe') -PassThru
        [void](Find-Window 'PulsarOverlay')
        Write-Host "Filling the graphs for $Warmup s"
        Park-Mouse
        Start-Sleep -Seconds $Warmup
    }

    if ('overlay' -in $Parts) {
        Write-Host 'Overlay'
        if (-not $OverlayShots.Contains('overlay-style-battery')) {
            Write-Warning 'no battery, so no battery cell capture'
        }
        if (Test-Wanted 'overlay-graph') { Save-Overlay 'overlay-graph' }
        foreach ($name in $OverlayShots.Keys) {
            if (-not (Test-Wanted $name)) { continue }
            $options = $OverlayShots[$name]
            Write-Config @options
            Send-ConfigChanged
            # The strip has only the pings since the item was turned on.
            if ($name -eq 'overlay-style-uptime') { Start-Sleep -Seconds 30 }
            Save-Overlay $name
        }
        foreach ($shape in 'rectangle', 'pill') {
            $name = if ($shape -eq 'pill') { 'overlay-floating-pill' } else { 'overlay-floating' }
            if (-not (Test-Wanted $name)) { continue }
            Write-Config -Floating -Shape $shape
            Send-ConfigChanged
            Park-Mouse
            Save-Around (Find-Window 'PulsarOverlay') $name 16
        }
    }

    if ('motion' -in $Parts) {
        Write-Host 'Motion'
        $notLoaded = @{ disk = 'area'; network = 'area' }
        $scenes = @(
            @{ Caption = 'Area graphs'; Options = @{} }
            @{ Caption = 'Icons'; Options = @{ Icons = $true } }
            @{ Caption = 'One-letter labels'; Options = @{ Labels = 'short' } }
            @{ Caption = 'Single-line graphs'; Options = @{ Inline = $true; Icons = $true } }
            @{ Caption = 'Vertical labels'; Options = @{ VerticalLabels = $true } }
            @{ Caption = 'Line'; Options = @{ Style = 'line' } }
            @{ Caption = 'Bars'; Options = @{ Style = 'bars' } }
            @{ Caption = 'Level meter'; Options = @{ Style = 'meter' } }
            @{ Caption = 'Segment meter'; Options = @{ Style = 'segments' } }
            @{ Caption = 'Spark underline'; Options = @{ Style = 'spark' } }
            @{ Caption = 'Ring'; Options = @{ Style = 'ring' } }
            @{ Caption = 'Dot matrix'; Options = @{ Style = 'dot_matrix' } }
            @{ Caption = 'Heat strip'; Options = @{ Style = 'heat_strip' } }
            @{ Caption = 'Bullet meter'; Options = @{ Style = 'bullet' } }
            @{ Caption = 'Range rail'; Options = @{ Style = 'range_rail' } }
            @{ Caption = 'Status tint'; Options = @{ Style = 'tint'; Styles = $notLoaded } }
            @{ Caption = 'Status bubble'; Options = @{ Style = 'bubble'; Styles = $notLoaded } }
            @{ Caption = 'Load with temperature'; Options = @{ Styles = @{ cpu = 'combined'; gpu = 'combined' } } }
            @{ Caption = 'Core bars and mirrored network'; Options = @{ Styles = @{ cpu = 'cores'; network = 'mirror_bars' } } }
            @{ Caption = 'Heat cells'; Options = @{ Styles = @{ cpu = 'heat' } } }
            @{ Caption = 'Mixed styles with trend arrows'; Options = @{ Styles = @{ cpu_temp = 'line'; ram = 'meter'; gpu = 'bars' }; TextOnly = 'disk'; Trend = $Showcase } }
            @{ Caption = 'Color by load'; Options = @{ Color = 'load' } }
            @{ Caption = 'Your own load colors'; Options = @{ Color = 'load'; LoadColors = @{ low = '#4CC2FF'; moderate = '#B18CFF'; high = '#FF8CC6'; critical = '#FF4D6A' } } }
            @{ Caption = 'Text'; Options = @{ Mode = 'text' } }
            @{ Caption = 'Text colored by load'; Options = @{ Mode = 'text'; Color = 'load' } }
        )
        $fps = 12
        $hold = 2.4
        $frames = Join-Path $out 'overlay-motion'
        Remove-Item -Recurse -Force $frames -ErrorAction SilentlyContinue
        New-Item -ItemType Directory $frames | Out-Null

        Write-Config
        Send-ConfigChanged
        Park-Mouse
        $overlay = Find-Window 'PulsarOverlay'
        $scale = [Native]::GetDpiForWindow($overlay) / 96.0
        $pad = [int](12 * $scale)
        $tb = Get-Rect (Find-Window 'Shell_TrayWnd')
        $right = (Get-Rect $overlay).Right + $pad
        # Wide enough for any style; cropped to the widest one seen.
        $left = [Math]::Max($tb.Left, $right - [int](900 * $scale))
        $minLeft = $right
        $bmp = New-Object System.Drawing.Bitmap ($right - $left), ($tb.Bottom - $tb.Top)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $msg = [Native]::RegisterWindowMessage('Pulsar.ConfigChanged')
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $n = 0
        for ($s = 0; $s -lt $scenes.Count; $s++) {
            if ($s) {
                $options = $scenes[$s].Options
                Write-Config @options
                [void][Native]::PostMessage([Native]::FindWindow('PulsarHost', [NullString]::Value), $msg, [IntPtr]::Zero, [IntPtr]::Zero)
            }
            while ($n / $fps -lt ($s + 1) * $hold) {
                $wait = [int]($n / $fps * 1000 - $clock.ElapsedMilliseconds)
                if ($wait -gt 0) { Start-Sleep -Milliseconds $wait }
                $g.CopyFromScreen($left, $tb.Top, 0, 0, $bmp.Size)
                $bmp.Save((Join-Path $frames ('{0:d4}.png' -f $n)), [System.Drawing.Imaging.ImageFormat]::Png)
                $bar = [Native]::FindWindow('Shell_TrayWnd', [NullString]::Value)
                $h = [Native]::FindWindowEx($bar, [IntPtr]::Zero, 'PulsarOverlay', [NullString]::Value)
                if ($h -ne [IntPtr]::Zero) { $minLeft = [Math]::Min($minLeft, (Get-Rect $h).Left) }
                $n++
            }
        }
        $g.Dispose(); $bmp.Dispose()

        $x = [Math]::Max(0, $minLeft - $pad - $left)
        $strip = [int](30 * $scale)
        $font = 'C\:/Windows/Fonts/seguisb.ttf'
        $filters = @("crop=iw-$($x):ih:$($x):0", "pad=iw:ih+$($strip):0:0:color=0x1b1b1d")
        for ($s = 0; $s -lt $scenes.Count; $s++) {
            $filters += "drawtext=fontfile='$font':text='$($scenes[$s].Caption)':fontcolor=0xe6e6e6:fontsize=$([int](14 * $scale)):x=(w-text_w)/2:y=h-$strip+($strip-text_h)/2:enable='between(t,$($s * $hold),$(($s + 1) * $hold))'"
        }
        $graph = ($filters -join ',') + ',split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle'
        $script = Join-Path $frames 'filter.txt'
        Set-Content $script $graph -Encoding utf8NoBOM
        if (Get-Command $Ffmpeg -ErrorAction SilentlyContinue) {
            & $Ffmpeg -y -loglevel error -framerate $fps -i (Join-Path $frames '%04d.png') -filter_complex_script $script (Join-Path $out 'overlay-motion.gif')
            if ($LASTEXITCODE) { throw 'ffmpeg failed' }
            Write-Host '  overlay-motion.gif'
        } else {
            Write-Warning "no $Ffmpeg, so the frames in $frames were not made into a GIF"
        }
    }

    if ($Light -or ($Parts | Where-Object { $_ -in 'popups', 'menus' })) {
        Write-Config -Show $All
        Send-ConfigChanged
        Update-Items
    }

    if ('popups' -in $Parts) {
        Write-Host 'Popups'
        $every = 0..($All.Count - 1)
        foreach ($detail in $Details) {
            if (-not @($every | Where-Object { Test-Wanted (Get-PopupName $_ "-$detail") }).Count) { continue }
            Write-Config -Show $All -Detail $detail
            Send-ConfigChanged
            Save-Popups $every "-$detail"
        }
        foreach ($material in $Materials) {
            if (-not @(0, 1 | Where-Object { Test-Wanted (Get-PopupName $_ "-$material") }).Count) { continue }
            Write-Config -Show $All -Background $material
            Send-ConfigChanged
            Save-Popups 0, 1 "-$material"
        }
    }

    if ('menus' -in $Parts) {
        Write-Host 'Menus'
        Save-Menu $items[0][0..1]
    }

    if ('widget' -in $Parts) {
        Write-Host 'Widget'
        foreach ($name in $WidgetShots.Keys) {
            if (-not (Test-Wanted $name)) { continue }
            $options = $WidgetShots[$name]
            Write-Config @options
            Send-ConfigChanged
            Park-Mouse
            $widget = Find-Window 'PulsarWidget'
            $backdrop = if ($options.WidgetMaterial -in 'glass', 'liquid', 'acrylic') { Show-Backdrop (Get-Rect $widget) }
            Save-Around $widget $name
            if ($backdrop) { $backdrop.Close(); $backdrop.Dispose() }
        }
        Write-Config
        Send-ConfigChanged
    }

    if ('game' -in $Parts -and (Test-Wanted 'game-strip')) {
        Write-Host 'Game strip'
        Write-Config -Games 'charmap.exe'
        Send-ConfigChanged
        $game = Start-Process charmap -PassThru
        try {
            # A window started from the background does not come to the front by itself.
            for ($i = 0; $i -lt 40 -and $game.MainWindowHandle -eq [IntPtr]::Zero; $i++) {
                Start-Sleep -Milliseconds 250
                $game.Refresh()
            }
            $r = Get-Rect $game.MainWindowHandle
            [void][Native]::SetCursorPos($r.Left + 100, $r.Top + 12)
            Start-Sleep -Milliseconds 300
            [Native]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
            [Native]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
            Park-Mouse
            Start-Sleep -Seconds 6
            $tb = Get-Rect (Find-Window 'Shell_TrayWnd')
            $strip = [Native]::Windows('PulsarOverlay') |
                Where-Object { (Get-Rect $_).Bottom -le $tb.Top } | Select-Object -First 1
            if ($strip) { Save-Around $strip 'game-strip' 0 } else { Write-Warning 'no game strip' }
        } finally {
            $game | Where-Object { -not $_.HasExited } | Stop-Process -Force
        }
        Write-Config
        Send-ConfigChanged
    }

    if ($Light) {
        Write-Host 'Light taskbar'
        Set-LightTaskbar 1
        if (Test-Wanted 'overlay-light') {
            Write-Config
            Send-ConfigChanged
            Save-Overlay 'overlay-light'
        }
        if (Test-Wanted 'overlay-text-light') {
            Write-Config -Mode text
            Send-ConfigChanged
            Save-Overlay 'overlay-text-light'
        }
        if (Test-Wanted (Get-PopupName 0 '-light')) {
            Write-Config -Show $All
            Send-ConfigChanged
            Save-Popups 0 '-light'
        }
        if (Test-Wanted (Get-PopupName 0 '-light-solid')) {
            Write-Config -Show $All -Background solid
            Send-ConfigChanged
            Save-Popups 0 '-light-solid'
        }
        Set-LightTaskbar $theme
    }

    if ('settings' -in $Parts) {
        Write-Host 'Settings'
        Write-Config
        $started += Start-Process (Join-Path $bin 'pulsar-settings.exe') -PassThru
        $settings = Find-Window $null 'Pulsar Settings'
        Start-Sleep -Seconds 2
        foreach ($page in $SettingsPages.Keys) {
            # Items scrolls the chosen item's options, beside the list.
            $wheelX = if ($page -eq 'items') { 740 } else { 500 }
            $views = if ($SettingsTabs.ContainsKey($page)) { $SettingsTabs[$page] } else { @{ '' = $null } }
            foreach ($tab in $views.Keys) {
                $name = if ($tab) { "settings-$page-$tab" } else { "settings-$page" }
                if (-not (Test-Wanted $name)) { continue }
                Send-Click $settings 82 $SettingsPages[$page]
                if ($null -ne $views[$tab]) { Send-Click $settings $views[$tab] $SettingsTabY }
                $previous = Save-Window $settings $name
                Write-Host "  $name"
                for ($n = 2; $n -lt 12; $n++) {
                    Send-Wheel $settings $wheelX 300 -4
                    $hash = Save-Window $settings "$name-$n"
                    if ($hash -eq $previous) {
                        Remove-Item (Join-Path $out "$name-$n.png")
                        break
                    }
                    Write-Host "  $name-$n"
                    $previous = $hash
                }
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
    if ($Only -and $kept -and (Test-Path $kept)) {
        Get-ChildItem $out -Filter *.png |
            Where-Object { $_.LastWriteTime -ge $runStarted -and -not (Test-WantedFile $_.BaseName) } |
            Remove-Item
        Get-ChildItem $kept -Filter *.png |
            Where-Object { -not (Test-Path (Join-Path $out $_.Name)) } |
            Copy-Item -Destination $out
    }
    $started | Where-Object { -not $_.HasExited } | Stop-Process -Force
    Get-Process pulsar-details -ErrorAction SilentlyContinue |
        Where-Object Path -eq (Join-Path $bin 'pulsar-details.exe') | Stop-Process -Force
    if ($Light) { Set-LightTaskbar $theme }
    $env:APPDATA = $saved.APPDATA
    $env:LOCALAPPDATA = $saved.LOCALAPPDATA
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

if ($Publish) {
    $readme = Get-Content (Join-Path $root 'README.md') -Raw
    Get-ChildItem $out -File | Where-Object Extension -in '.png', '.gif' | ForEach-Object {
        $target = Join-Path $root "docs\images\$($_.Name)"
        $name = $_.BaseName
        $new = -not (Test-Path $target) -and $readme.Contains("docs/images/$($_.Name)")
        $changed = (Test-Path $target) -and ($Replace | Where-Object { $name -like $_ })
        if ($new -or $changed) { Copy-Item $_.FullName $target }
    }
}
Write-Host "Saved to $out"
