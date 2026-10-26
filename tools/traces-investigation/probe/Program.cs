// Зонд расследования следов (задача 0.6). См. t06probe.csproj.
// Вывод — одна строка JSON на stdout (WinExe: stdout перенаправляет вызывающий).
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;
using Microsoft.Web.WebView2.Core;

namespace T06Probe;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        if (args.Length == 0) return Usage();
        try
        {
            return args[0] switch
            {
                "creds" => Creds.Dump(),
                "dialog" when args.Length >= 3 => FolderDialog.Run(args[1], args[2]),
                "webview" when args.Length >= 3 => MiniWebView.Run(args[1], int.Parse(args[2])),
                _ => Usage(),
            };
        }
        catch (Exception e)
        {
            Out.Json(new { error = e.ToString() });
            return 2;
        }
    }

    private static int Usage()
    {
        Out.Json(new { error = "usage: creds | dialog <variant> <folder> | webview <rhizomind-flags|default> <seconds>" });
        return 64;
    }
}

internal static class Out
{
    public static void Json(object o)
    {
        Console.Out.WriteLine(JsonSerializer.Serialize(o));
        Console.Out.Flush();
    }
}

// --- диспетчер учётных данных ------------------------------------------------

internal static class Creds
{
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct CREDENTIAL
    {
        public uint Flags;
        public uint Type;
        public string TargetName;
        public string Comment;
        public long LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist;
        public uint AttributeCount;
        public IntPtr Attributes;
        public string TargetAlias;
        public string UserName;
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredEnumerateW(string filter, uint flags, out uint count, out IntPtr credentials);

    [DllImport("advapi32.dll")]
    private static extern void CredFree(IntPtr buffer);

    public static int Dump()
    {
        var list = new List<object>();
        // CRED_ENUMERATE_ALL_CREDENTIALS = 1
        if (CredEnumerateW(null, 1, out var count, out var ptr))
        {
            for (var i = 0; i < count; i++)
            {
                var item = Marshal.ReadIntPtr(ptr, i * IntPtr.Size);
                var c = Marshal.PtrToStructure<CREDENTIAL>(item);
                list.Add(new
                {
                    target = c.TargetName,
                    type = c.Type,
                    persist = c.Persist,
                    user = c.UserName,
                    comment = c.Comment,
                    blobSize = c.CredentialBlobSize,
                    attributes = c.AttributeCount,
                    lastWritten = DateTime.FromFileTimeUtc(c.LastWritten).ToString("o"),
                });
            }
            CredFree(ptr);
        }
        else
        {
            list.Add(new { enumerateError = Marshal.GetLastWin32Error() });
        }
        Out.Json(new { credentials = list });
        return 0;
    }
}

// --- окно выбора папки -------------------------------------------------------

[ComImport, Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IShellItem
{
    void BindToHandler(IntPtr pbc, ref Guid bhid, ref Guid riid, out IntPtr ppv);
    void GetParent(out IShellItem ppsi);
    void GetDisplayName(uint sigdnName, [MarshalAs(UnmanagedType.LPWStr)] out string ppszName);
    void GetAttributes(uint sfgaoMask, out uint psfgaoAttribs);
    void Compare(IShellItem psi, uint hint, out int piOrder);
}

// IFileOpenDialog со всеми методами базовых интерфейсов по порядку vtable:
// IModalWindow::Show, затем IFileDialog, затем свои.
[ComImport, Guid("d57c7288-d4ad-4768-be02-9d969532d960"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
internal interface IFileOpenDialog
{
    [PreserveSig] int Show(IntPtr parent);
    void SetFileTypes(uint cFileTypes, IntPtr rgFilterSpec);
    void SetFileTypeIndex(uint iFileType);
    void GetFileTypeIndex(out uint piFileType);
    void Advise(IntPtr pfde, out uint pdwCookie);
    void Unadvise(uint dwCookie);
    void SetOptions(uint fos);
    void GetOptions(out uint pfos);
    void SetDefaultFolder(IShellItem psi);
    void SetFolder(IShellItem psi);
    void GetFolder(out IShellItem ppsi);
    void GetCurrentSelection(out IShellItem ppsi);
    void SetFileName([MarshalAs(UnmanagedType.LPWStr)] string pszName);
    void GetFileName([MarshalAs(UnmanagedType.LPWStr)] out string pszName);
    void SetTitle([MarshalAs(UnmanagedType.LPWStr)] string pszTitle);
    void SetOkButtonLabel([MarshalAs(UnmanagedType.LPWStr)] string pszText);
    void SetFileNameLabel([MarshalAs(UnmanagedType.LPWStr)] string pszLabel);
    void GetResult(out IShellItem ppsi);
    void AddPlace(IShellItem psi, int fdap);
    void SetDefaultExtension([MarshalAs(UnmanagedType.LPWStr)] string pszDefaultExtension);
    void Close(int hr);
    void SetClientGuid(ref Guid guid);
    [PreserveSig] int ClearClientData();
    void SetFilter(IntPtr pFilter);
    void GetResults(out IntPtr ppenum);
    void GetSelectedItems(out IntPtr ppsai);
}

[ComImport, Guid("DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7")]
internal class FileOpenDialogCoClass
{
}

internal static class FolderDialog
{
    private const uint FOS_NOCHANGEDIR = 0x8;
    private const uint FOS_PICKFOLDERS = 0x20;
    private const uint FOS_FORCEFILESYSTEM = 0x40;
    private const uint FOS_DONTADDTORECENT = 0x2000000;
    private const uint SIGDN_FILESYSPATH = 0x80058000;
    private static readonly Guid ClientGuid = new("7a2f4c1e-9b3d-4e5f-8a6b-0c1d2e3f4a5b");

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
    private static extern void SHCreateItemFromParsingName(string path, IntPtr pbc, ref Guid riid, out IShellItem item);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr FindWindowW(string cls, string title);

    [DllImport("user32.dll")]
    private static extern IntPtr GetDlgItem(IntPtr dlg, int id);

    [DllImport("user32.dll")]
    private static extern bool PostMessageW(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);

    [DllImport("user32.dll")]
    private static extern IntPtr SendMessageW(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    private static extern bool IsWindow(IntPtr hwnd);

    private const uint WM_COMMAND = 0x0111;
    private const uint BM_CLICK = 0x00F5;
    private const int IDOK = 1;
    private const int IDCANCEL = 2;

    /// Варианты:
    ///   rfd             — ровно как rfd 0.16.1: SetOptions(FOS_PICKFOLDERS), SetTitle, SetFolder;
    ///   rfd-cancel      — то же, но пользователь нажимает «Отмена»;
    ///   dontaddtorecent — + FOS_DONTADDTORECENT;
    ///   clientguid      — + SetClientGuid;
    ///   clientguid-clear— + SetClientGuid, после Show — ClearClientData;
    ///   proposal        — FOS_PICKFOLDERS|FOS_DONTADDTORECENT + SetClientGuid + ClearClientData (предложение);
    ///   all             — FOS_PICKFOLDERS|FOS_DONTADDTORECENT|FOS_NOCHANGEDIR|FOS_FORCEFILESYSTEM + SetClientGuid + ClearClientData.
    public static int Run(string variant, string folder)
    {
        uint options = FOS_PICKFOLDERS;
        var guid = false;
        var clear = false;
        var cancel = false;
        switch (variant)
        {
            case "rfd": break;
            case "rfd-cancel": cancel = true; break;
            case "dontaddtorecent": options |= FOS_DONTADDTORECENT; break;
            case "clientguid": guid = true; break;
            case "clientguid-clear": guid = true; clear = true; break;
            case "proposal":
                options |= FOS_DONTADDTORECENT;
                guid = true;
                clear = true;
                break;
            case "all":
                options |= FOS_DONTADDTORECENT | FOS_NOCHANGEDIR | FOS_FORCEFILESYSTEM;
                guid = true;
                clear = true;
                break;
            default: throw new ArgumentException("unknown variant " + variant);
        }

        var title = "t06 probe " + variant + " " + Environment.ProcessId;
        var full = Path.GetFullPath(folder);
        var parent = Path.GetDirectoryName(full);
        var leaf = Path.GetFileName(full);

        var dialog = (IFileOpenDialog)new FileOpenDialogCoClass();
        if (guid)
        {
            var g = ClientGuid;
            dialog.SetClientGuid(ref g);
        }
        dialog.SetOptions(options);
        dialog.SetTitle(title);
        var iid = typeof(IShellItem).GUID;
        SHCreateItemFromParsingName(parent, IntPtr.Zero, ref iid, out var parentItem);
        dialog.SetFolder(parentItem);
        // Пользователь стоит в родительской папке и выбирает папку хранилища по имени.
        dialog.SetFileName(leaf);

        var log = new List<string>();
        var clicker = new Thread(() =>
        {
            IntPtr h = IntPtr.Zero;
            for (var i = 0; i < 200 && h == IntPtr.Zero; i++)
            {
                Thread.Sleep(100);
                h = FindWindowW("#32770", title);
            }
            if (h == IntPtr.Zero) { log.Add("dialog window not found"); return; }
            log.Add("dialog window found");
            Thread.Sleep(2000); // окно успевает загрузить папку, как у пользователя
            SetForegroundWindow(h);
            var id = cancel ? IDCANCEL : IDOK;
            PostMessageW(h, WM_COMMAND, (IntPtr)id, GetDlgItem(h, id));
            log.Add("posted WM_COMMAND " + id);
            Thread.Sleep(3000);
            if (IsWindow(h))
            {
                var button = GetDlgItem(h, id);
                SendMessageW(button, BM_CLICK, IntPtr.Zero, IntPtr.Zero);
                log.Add("sent BM_CLICK " + id);
                Thread.Sleep(3000);
            }
            if (IsWindow(h))
            {
                PostMessageW(h, WM_COMMAND, (IntPtr)IDCANCEL, IntPtr.Zero);
                log.Add("still open: posted IDCANCEL");
            }
        }) { IsBackground = true };
        clicker.Start();

        var hr = dialog.Show(IntPtr.Zero);
        string chosen = null;
        if (hr == 0)
        {
            dialog.GetResult(out var item);
            item.GetDisplayName(SIGDN_FILESYSPATH, out chosen);
        }
        int? clearHr = clear ? dialog.ClearClientData() : null;
        clicker.Join(1000);
        Marshal.FinalReleaseComObject(dialog);
        Out.Json(new
        {
            variant,
            options = "0x" + options.ToString("x"),
            clientGuid = guid,
            clearClientData = clear,
            clearHr = clearHr.HasValue ? "0x" + clearHr.Value.ToString("x8") : null,
            hr = "0x" + hr.ToString("x8"),
            chosen,
            expected = full,
            log,
        });
        return 0;
    }
}

// --- минимальное окно WebView2 без ядра Rhizomind ----------------------------

internal static class MiniWebView
{
    // Флаги процесса браузера выпускной сборки Rhizomind (командная строка из
    // тестов следов CI, без служебных, которые движок ставит сам).
    private const string RhizomindFlags =
        "--noerrdialogs --disable-background-networking --disable-component-update " +
        "--disable-domain-reliability --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection " +
        "--host-resolver-rules=\"MAP * ~NOTFOUND\" --no-pings --no-proxy-server";

    public static int Run(string mode, int seconds)
    {
        // mode: <rhizomind-flags|default>[+nav][+lang]
        //   +nav  — страница с http://tauri.localhost/ через WebResourceRequested (как Tauri на Windows);
        //   +lang — Language = en-US и AreBrowserExtensionsEnabled = false (как wry).
        var parts = mode.Split('+');
        var flags = parts[0] switch
        {
            "rhizomind-flags" => RhizomindFlags,
            "default" => null,
            _ => throw new ArgumentException("unknown mode " + mode),
        };
        var nav = Array.IndexOf(parts, "nav") > 0;
        var lang = Array.IndexOf(parts, "lang") > 0;
        var dataDir = Path.Combine(Path.GetTempPath(), "t06-webview-" + Environment.ProcessId + "-" + Guid.NewGuid().ToString("N")[..8]);
        Directory.CreateDirectory(dataDir);
        var info = new Dictionary<string, object> { ["mode"] = mode, ["dataDir"] = dataDir };
        uint browserPid = 0;
        var browserExited = new ManualResetEventSlim(false);

        Application.EnableVisualStyles();
        var form = new Form { Text = "t06 mini webview " + mode, Width = 800, Height = 600 };
        form.Load += async (_, _) =>
        {
            try
            {
                var options = new CoreWebView2EnvironmentOptions(flags)
                {
                    AllowSingleSignOnUsingOSPrimaryAccount = false,
                    IsCustomCrashReportingEnabled = true,
                };
                if (lang)
                {
                    options.Language = "en-US";
                    options.AreBrowserExtensionsEnabled = false;
                }
                var env = await CoreWebView2Environment.CreateAsync(null, dataDir, options);
                env.BrowserProcessExited += (_, _) => browserExited.Set();
                var controllerOptions = env.CreateCoreWebView2ControllerOptions();
                controllerOptions.IsInPrivateModeEnabled = true;
                var controller = await env.CreateCoreWebView2ControllerAsync(form.Handle, controllerOptions);
                controller.Bounds = form.ClientRectangle;
                var core = controller.CoreWebView2;
                core.Settings.IsPasswordAutosaveEnabled = false;
                core.Settings.IsGeneralAutofillEnabled = false;
                browserPid = core.BrowserProcessId;
                info["browserPid"] = browserPid;
                info["runtime"] = env.BrowserVersionString;
                info["inPrivate"] = core.Profile.IsInPrivateModeEnabled;
                if (nav)
                {
                    core.AddWebResourceRequestedFilter("http://tauri.localhost/*", CoreWebView2WebResourceContext.All);
                    core.WebResourceRequested += (_, e) =>
                    {
                        var html = Encoding.UTF8.GetBytes("<!doctype html><title>t06</title><h1>t06 mini webview (nav)</h1>");
                        e.Response = env.CreateWebResourceResponse(new MemoryStream(html), 200, "OK", "Content-Type: text/html; charset=utf-8");
                    };
                    core.Navigate("http://tauri.localhost/");
                }
                else
                {
                    core.NavigateToString("<!doctype html><title>t06</title><h1>t06 mini webview</h1>");
                }
                var timer = new System.Windows.Forms.Timer { Interval = seconds * 1000 };
                timer.Tick += (_, _) => { timer.Stop(); controller.Close(); form.Close(); };
                timer.Start();
            }
            catch (Exception e)
            {
                info["error"] = e.ToString();
                form.Close();
            }
        };
        Application.Run(form);
        info["browserExited"] = browserExited.Wait(TimeSpan.FromSeconds(20));
        for (var i = 0; i < 20 && Directory.Exists(dataDir); i++)
        {
            try { Directory.Delete(dataDir, true); }
            catch { Thread.Sleep(500); }
        }
        info["dataDirRemoved"] = !Directory.Exists(dataDir);
        Out.Json(info);
        return 0;
    }
}
