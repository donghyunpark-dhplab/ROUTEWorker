param([string]$Exe, [string]$Out, [int]$WaitMs = 5000, [switch]$Keep, [string]$Clicks = "")

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Win {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
'@
[Win]::SetProcessDPIAware() | Out-Null

$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Milliseconds $WaitMs
$p.Refresh()
if ($p.HasExited) { "process exited early with code $($p.ExitCode)"; exit 1 }
$h = $p.MainWindowHandle
if ($h -eq [IntPtr]::Zero) { "no main window"; if (-not $Keep) { $p.Kill() }; exit 1 }

$r = New-Object Win+RECT
[Win]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $ht = $r.B - $r.T
# Synthetic mouse messages posted to the window only (does not move the real cursor).
# Coordinates are given in screenshot pixels: "L:x,y;R:x,y;D:x,y" (left, right, double).
if ($Clicks) {
    $o = New-Object Win+POINT
    [Win]::ClientToScreen($h, [ref]$o) | Out-Null
    $dx = $o.X - $r.L; $dy = $o.Y - $r.T
    foreach ($c in $Clicks.Split(';')) {
        $kind, $xy = $c.Split(':', 2)
        if ($kind -eq 'T') {
            foreach ($ch in $xy.ToCharArray()) { [Win]::PostMessage($h, 0x102, [IntPtr][int]$ch, [IntPtr]1) | Out-Null; Start-Sleep -Milliseconds 30 }
            Start-Sleep -Milliseconds 300; continue
        }
        $x, $y = $xy.Split(',')
        $lp = [IntPtr]((([int]$y - $dy) -shl 16) -bor (([int]$x - $dx) -band 0xFFFF))
        [Win]::PostMessage($h, 0x200, [IntPtr]0, $lp) | Out-Null; Start-Sleep -Milliseconds 150
        $n = 1; if ($kind -eq 'D') { $n = 2 }
        for ($i = 0; $i -lt $n; $i++) {
            if ($kind -eq 'R') { $down = 0x204; $up = 0x205; $wp = 2 } else { $down = 0x201; $up = 0x202; $wp = 1 }
            [Win]::PostMessage($h, $down, [IntPtr]$wp, $lp) | Out-Null; Start-Sleep -Milliseconds 60
            [Win]::PostMessage($h, $up, [IntPtr]0, $lp) | Out-Null; Start-Sleep -Milliseconds 60
        }
        Start-Sleep -Milliseconds 500
    }
}
$bmp = New-Object System.Drawing.Bitmap $w, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [Win]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc); $g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
"captured ${w}x${ht} printwindow=$ok pid=$($p.Id)"
if (-not $Keep) { $p.Kill() }
