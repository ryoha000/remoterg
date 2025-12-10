using System;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using WinRT.Interop;

namespace Ui;

public sealed partial class MainWindow : Window, INotifyPropertyChanged
{
    public ObservableCollection<WindowEntry> Windows { get; } = new();

    private WindowEntry? _selectedWindow;
    public WindowEntry? SelectedWindow
    {
        get => _selectedWindow;
        set
        {
            if (SetProperty(ref _selectedWindow, value, nameof(SelectedWindow)))
            {
                OnPropertyChanged(nameof(HasSelection));
            }
        }
    }

    public bool HasSelection => SelectedWindow is not null;

    private string _statusMessage = "未選択";
    public string StatusMessage
    {
        get => _statusMessage;
        set => SetProperty(ref _statusMessage, value, nameof(StatusMessage));
    }

    public MainWindow()
    {
        InitializeComponent();
        RefreshWindows();
    }

    private void OnRefreshClick(object sender, RoutedEventArgs e)
    {
        RefreshWindows();
    }

    private void OnStartHostdClick(object sender, RoutedEventArgs e)
    {
        if (SelectedWindow is null)
        {
            StatusMessage = "ウィンドウを選択してください";
            return;
        }

        var hostdPath = ResolveHostdPath();
        if (!File.Exists(hostdPath))
        {
            StatusMessage = $"hostd.exe が見つかりません: {hostdPath}";
            return;
        }

        var psi = new ProcessStartInfo
        {
            FileName = hostdPath,
            Arguments = $"--hwnd {SelectedWindow.HandleValue}",
            WorkingDirectory = Path.GetDirectoryName(hostdPath) ?? AppContext.BaseDirectory,
            UseShellExecute = false,
        };

        try
        {
            Process.Start(psi);
            StatusMessage = $"hostd を起動しました (hwnd=0x{SelectedWindow.HandleValue:X})";
        }
        catch (Exception ex)
        {
            StatusMessage = $"hostd 起動に失敗: {ex.Message}";
        }
    }

    private void RefreshWindows()
    {
        Windows.Clear();

        var shellWindow = GetShellWindow();
        var currentWindow = WindowNative.GetWindowHandle(this);

        EnumWindows((hWnd, _) =>
        {
            if (hWnd == IntPtr.Zero || hWnd == shellWindow || hWnd == currentWindow)
            {
                return true;
            }

            if (!IsWindowVisible(hWnd))
            {
                return true;
            }

            var length = GetWindowTextLengthW(hWnd);
            if (length == 0)
            {
                return true;
            }

            var titleSb = new StringBuilder(length + 1);
            GetWindowTextW(hWnd, titleSb, titleSb.Capacity);
            var title = titleSb.ToString();
            if (string.IsNullOrWhiteSpace(title))
            {
                return true;
            }

            var classSb = new StringBuilder(256);
            GetClassNameW(hWnd, classSb, classSb.Capacity);

            GetWindowThreadProcessId(hWnd, out var pid);

            Windows.Add(new WindowEntry
            {
                Handle = hWnd,
                Title = title,
                ClassName = classSb.ToString(),
                ProcessId = (int)pid,
            });

            return true;
        }, IntPtr.Zero);

        StatusMessage = $"ウィンドウ数: {Windows.Count}";
    }

    private static string ResolveHostdPath()
    {
        // ビルド後に同梱された hostd.exe を参照
        return Path.Combine(AppContext.BaseDirectory, "hostd.exe");
    }

    #region Win32 interop

    private delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);

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

    #endregion

    #region INotifyPropertyChanged helpers

    public event PropertyChangedEventHandler? PropertyChanged;

    private void OnPropertyChanged(string propertyName)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }

    private bool SetProperty<T>(ref T storage, T value, string propertyName)
    {
        if (Equals(storage, value))
        {
            return false;
        }

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
    public string DetailText => $"HWND=0x{HandleValue:X} PID={ProcessId} Class={ClassName}";
}
