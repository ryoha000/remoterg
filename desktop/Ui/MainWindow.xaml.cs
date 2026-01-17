using System;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using WinRT.Interop;

namespace Ui;

public sealed partial class MainWindow : Window, INotifyPropertyChanged
{
    public ObservableCollection<WindowEntry> Windows { get; } = new();
    public ObservableCollection<ScreenEntry> Screens { get; } = new();

    private WindowEntry? _selectedWindow;
    public WindowEntry? SelectedWindow
    {
        get => _selectedWindow;
        set
        {
            if (SetProperty(ref _selectedWindow, value, nameof(SelectedWindow)))
            {
                if (value != null)
                {
                    SelectedScreen = null; // Deselect screen if window is selected
                }
                OnPropertyChanged(nameof(CanStart));
                UpdateStatus();
            }
        }
    }

    private ScreenEntry? _selectedScreen;
    public ScreenEntry? SelectedScreen
    {
        get => _selectedScreen;
        set
        {
            if (SetProperty(ref _selectedScreen, value, nameof(SelectedScreen)))
            {
                if (value != null)
                {
                    SelectedWindow = null; // Deselect window if screen is selected
                }
                OnPropertyChanged(nameof(CanStart));
                UpdateStatus();
            }
        }
    }

    public bool CanStart => SelectedWindow != null || SelectedScreen != null;

    private string _statusMessage = "Ready";
    public string StatusMessage
    {
        get => _statusMessage;
        set => SetProperty(ref _statusMessage, value, nameof(StatusMessage));
    }

    public MainWindow()
    {
        InitializeComponent();
        RefreshWindows();
        // Set initial status
        UpdateStatus();
    }

    private void OnRefreshClick(object sender, RoutedEventArgs e)
    {
        RefreshWindows();
        UpdateStatus();
    }

    private void UpdateStatus()
    {
        if (SelectedWindow != null)
        {
            StatusMessage = $"Selected Window: {SelectedWindow.Title} (HWND: 0x{SelectedWindow.HandleValue:X})";
        }
        else if (SelectedScreen != null)
        {
            StatusMessage = $"Selected Screen: {SelectedScreen.DeviceName} (HMON: 0x{SelectedScreen.HandleValue:X})";
        }
        else
        {
            StatusMessage = $"Found {Windows.Count} windows and {Screens.Count} screens.";
        }
    }

    private void OnStartHostdClick(object sender, RoutedEventArgs e)
    {
        long handle = 0;
        string type = "";

        if (SelectedWindow != null)
        {
            handle = SelectedWindow.HandleValue;
            type = "Window";
        }
        else if (SelectedScreen != null)
        {
            handle = SelectedScreen.HandleValue;
            type = "Screen";
        }
        else
        {
            StatusMessage = "Please select a target first.";
            return;
        }

        var hostdPath = ResolveHostdPath();
        if (!File.Exists(hostdPath))
        {
            StatusMessage = $"hostd.exe NOT found at: {hostdPath}";
            return;
        }

        var psi = new ProcessStartInfo
        {
            FileName = hostdPath,
            Arguments = $"--hwnd {handle}", // Passing HMONITOR as HWND might not work yet in hostd, enabling as planned
            WorkingDirectory = Path.GetDirectoryName(hostdPath) ?? AppContext.BaseDirectory,
            UseShellExecute = false,
        };

        try
        {
            Process.Start(psi);
            StatusMessage = $"Started hostd for {type} (Handle=0x{handle:X})";
        }
        catch (Exception ex)
        {
            StatusMessage = $"Failed to start hostd: {ex.Message}";
        }
    }

    private void RefreshWindows()
    {
        Windows.Clear();
        Screens.Clear();

        // 1. Refresh Windows
        var shellWindow = GetShellWindow();
        var currentWindow = WindowNative.GetWindowHandle(this);

        EnumWindows((hWnd, _) =>
        {
            if (hWnd == IntPtr.Zero || hWnd == shellWindow || hWnd == currentWindow) return true;
            if (!IsWindowVisible(hWnd)) return true;
            
            var length = GetWindowTextLengthW(hWnd);
            if (length == 0) return true;

            var titleSb = new StringBuilder(length + 1);
            GetWindowTextW(hWnd, titleSb, titleSb.Capacity);
            var title = titleSb.ToString();
            if (string.IsNullOrWhiteSpace(title)) return true;

            var classSb = new StringBuilder(256);
            GetClassNameW(hWnd, classSb, classSb.Capacity);
            
            // Filter out some common clutter
            var className = classSb.ToString();
            if (className == "Progman" || className == "Windows.UI.Core.CoreWindow") return true;

            GetWindowThreadProcessId(hWnd, out var pid);

            Windows.Add(new WindowEntry
            {
                Handle = hWnd,
                Title = title,
                ClassName = className,
                ProcessId = (int)pid,
            });

            return true;
        }, IntPtr.Zero);

        // 2. Refresh Screens
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr hMonitor, IntPtr hdcMonitor, ref Rect lprcMonitor, IntPtr dwData) =>
        {
            var mi = new MonitorInfoEx();
            mi.Size = Marshal.SizeOf(mi);
            if (GetMonitorInfo(hMonitor, ref mi))
            {
                Screens.Add(new ScreenEntry
                {
                    Handle = hMonitor,
                    DeviceName = mi.DeviceName,
                    Resolution = $"{mi.Monitor.Right - mi.Monitor.Left}x{mi.Monitor.Bottom - mi.Monitor.Top}",
                    IsPrimary = (mi.Flags & 1) != 0 // MONITORINFOF_PRIMARY
                });
            }
            return true;
        }, IntPtr.Zero);

        UpdateStatus();
    }

    private static string ResolveHostdPath()
    {
        return Path.Combine(AppContext.BaseDirectory, "hostd.exe");
    }

    #region Win32 interop

    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    private delegate bool MonitorEnumDelegate(IntPtr hMonitor, IntPtr hdcMonitor, ref Rect lprcMonitor, IntPtr dwData);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr lprcClip, MonitorEnumDelegate lpfnEnum, IntPtr dwData);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern int GetWindowTextW(IntPtr hWnd, StringBuilder lpString, int nMaxCount);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern int GetWindowTextLengthW(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    private static extern IntPtr GetShellWindow();

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern int GetClassNameW(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    private static extern bool GetMonitorInfo(IntPtr hMonitor, ref MonitorInfoEx lpmi);

    [StructLayout(LayoutKind.Sequential)]
    private struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    private struct MonitorInfoEx
    {
        public int Size;
        public Rect Monitor;
        public Rect WorkArea;
        public uint Flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)]
        public string DeviceName;
    }

    #endregion

    #region INotifyPropertyChanged helpers

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged(string propertyName)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }

    private bool SetProperty<T>(ref T storage, T value, string propertyName)
    {
        if (Equals(storage, value)) return false;
        storage = value;
        OnPropertyChanged(propertyName);
        return true;
    }

    #endregion
}

public class WindowEntry
{
    public IntPtr Handle { get; set; }
    public string Title { get; set; } = string.Empty;
    public string ClassName { get; set; } = string.Empty;
    public int ProcessId { get; set; }
    public long HandleValue => Handle.ToInt64();
    public string DetailText => $"PID={ProcessId}";
    
    // Placeholder for Icon logic (future)
    public ImageSource? IconSource { get; set; }
    public Visibility IconVisibility => IconSource != null ? Visibility.Visible : Visibility.Collapsed;
}

public class ScreenEntry
{
    public IntPtr Handle { get; set; }
    public string DeviceName { get; set; } = string.Empty;
    public string Resolution { get; set; } = string.Empty;
    public bool IsPrimary { get; set; }
    public long HandleValue => Handle.ToInt64();
}
