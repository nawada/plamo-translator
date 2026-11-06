param([string]$Exe, [string]$OutDir)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class E {
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
 [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
 [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint c, uint t);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr p);
 [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool f);
 [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
 [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
 public static bool Force(IntPtr h) { IntPtr fg = GetForegroundWindow(); uint t = GetWindowThreadProcessId(fg, IntPtr.Zero); uint c = GetCurrentThreadId();
   bool at = t != 0 && t != c && AttachThreadInput(c, t, true); BringWindowToTop(h); SetForegroundWindow(h); if (at) AttachThreadInput(c, t, false); return GetForegroundWindow() == h; }
 public struct RECT { public int L, T, R, B; } }
"@
[E]::SetProcessDPIAware() | Out-Null
function Pump($ms) { $end = (Get-Date).AddMilliseconds($ms); while ((Get-Date) -lt $end) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 } }
function Shot($h, $name) {
  [E]::Force($h) | Out-Null; Pump 400
  $r = New-Object E+RECT; [E]::GetWindowRect($h, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size); $bmp.Save("$OutDir\$name"); $g.Dispose(); $bmp.Dispose()
}
function Send($vk, $up) { $sc = [byte][E]::MapVirtualKey($vk, 0); [E]::keybd_event($vk, $sc, $(if ($up) {2} else {0}), [UIntPtr]::Zero) }

$data = "$env:LOCALAPPDATA\jp.plamo-translater.desktop"
$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 5
$p.Refresh(); $app = $p.MainWindowHandle

# Click the install button (located from the earlier screenshot: x=838, y=157 in window coords)
[E]::Force($app) | Out-Null; Pump 300
$r = New-Object E+RECT; [E]::GetWindowRect($app, [ref]$r) | Out-Null
[E]::SetCursorPos($r.L + 838, $r.T + 157) | Out-Null; Pump 100
[E]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [E]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
Pump 8000
Shot $app "e2e-1-downloading.png"

# Wait for llama-server to become ready (model file present + listening)
$deadline = (Get-Date).AddMinutes(10); $ready = $false
while ((Get-Date) -lt $deadline) {
  $log = "$data\logs\llama-server.log"
  if ((Test-Path $log) -and (Select-String -Path $log -Pattern "listening on" -Quiet)) { $ready = $true; break }
  Pump 2000
}
"engine ready: $ready"
Pump 2000
Shot $app "e2e-2-ready.png"

# Select text in a separate window and press Ctrl+K
$f = New-Object System.Windows.Forms.Form
$f.Text = "HotkeyTestSource"; $f.Width = 700; $f.Height = 200; $f.TopMost = $true
$tb = New-Object System.Windows.Forms.TextBox
$tb.Multiline = $true; $tb.Dock = "Fill"; $tb.Font = New-Object System.Drawing.Font("Segoe UI", 14)
$tb.Text = "The meeting has been rescheduled to next Monday. Please open the project dashboard to see the updated agenda."
$f.Controls.Add($tb); $f.Show(); Pump 500
[E]::Force($f.Handle) | Out-Null; $f.Activate(); $tb.Focus() | Out-Null; $tb.SelectAll(); Pump 500
"foreground is test form: " + ([E]::GetForegroundWindow() -eq $f.Handle)
Send 0x11 $false; Send 0x4B $false; Start-Sleep -m 50; Send 0x4B $true; Send 0x11 $true
Pump 6000
"foreground is app: " + ([E]::GetForegroundWindow() -eq $app)
$f.Hide(); Pump 300
Shot $app "e2e-3-translated.png"
$f.Close()
Stop-Process -Id $p.Id
"---- app.log ----"; Get-Content "$data\logs\app.log" -Tail 10
"---- llama-server.log (tail) ----"; Get-Content "$data\logs\llama-server.log" -Tail 15
"---- files ----"; Get-ChildItem -Recurse "$data\models", "$data\llama.cpp" -Include *.gguf, llama-server.exe | Select FullName, Length
