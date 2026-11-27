import { spawn, type ChildProcess } from 'node:child_process';
import { pathToFileURL } from 'node:url';

// Независимый владелец системного буфера передаёт нарисованный пиксель.
// В X11 он обслуживает запрос получения байтов до окончания вставки.
export const copiedPath = process.platform === 'win32' ? 'C:/external/rhizomind-fixture.png' : '/external/rhizomind-fixture.png';

export async function setClipboard(kind: 'image' | 'path' | 'files' | 'mixed', value = copiedPath): Promise<ChildProcess> {
  const windows = process.platform === 'win32';
  const env = { ...process.env, RHIZOMIND_CLIPBOARD_TEXT: value, RHIZOMIND_CLIPBOARD_URI: pathToFileURL(value).href };
  const win = [
    "$ErrorActionPreference='Stop'", 'Add-Type -AssemblyName System.Windows.Forms', 'Add-Type -AssemblyName System.Drawing',
    ...(kind === 'image' ? [
      '$image = New-Object System.Drawing.Bitmap 2,2',
      '$image.SetPixel(0,0,[System.Drawing.Color]::Red)',
      '[System.Windows.Forms.Clipboard]::SetImage($image)',
    ] : kind === 'path' ? ['[System.Windows.Forms.Clipboard]::SetText($env:RHIZOMIND_CLIPBOARD_TEXT)'] : [
      '$items=New-Object System.Collections.Specialized.StringCollection',
      '[void]$items.Add($env:RHIZOMIND_CLIPBOARD_TEXT)',
      ...(kind === 'mixed' ? [
        '$data=New-Object System.Windows.Forms.DataObject', '$data.SetFileDropList($items)',
        '$image=New-Object System.Drawing.Bitmap 2,2', '$data.SetImage($image)',
        '[System.Windows.Forms.Clipboard]::SetDataObject($data,$true)',
      ] : ['[System.Windows.Forms.Clipboard]::SetFileDropList($items)']),
    ]),
    "Write-Output ('formats=' + ([System.Windows.Forms.Clipboard]::GetDataObject().GetFormats($false) -join ','))",
    "Write-Output 'ready'",
  ].join('; ');
  const linux = [
    'import ctypes as c, os', "gtk=c.CDLL('libgtk-3.so.0')", "gdk=c.CDLL('libgdk-3.so.0')", "pix=c.CDLL('libgdk_pixbuf-2.0.so.0')",
    'gtk.gtk_init(None,None)', 'gdk.gdk_atom_intern_static_string.restype=c.c_void_p',
    'gtk.gtk_clipboard_get.argtypes=[c.c_void_p]', 'gtk.gtk_clipboard_get.restype=c.c_void_p',
    "cb=gtk.gtk_clipboard_get(gdk.gdk_atom_intern_static_string(b'CLIPBOARD'))",
    ...(kind === 'image' ? [
      'pix.gdk_pixbuf_new.restype=c.c_void_p', 'p=pix.gdk_pixbuf_new(0,0,8,2,2)',
      'pix.gdk_pixbuf_fill.argtypes=[c.c_void_p,c.c_uint32]', 'pix.gdk_pixbuf_fill(p,0xff0000ff)',
      'gtk.gtk_clipboard_set_image.argtypes=[c.c_void_p,c.c_void_p]', 'gtk.gtk_clipboard_set_image(cb,p)',
    ] : kind === 'files' || kind === 'mixed' ? [
      "class Target(c.Structure): _fields_=[('name', c.c_char_p), ('flags', c.c_uint), ('info', c.c_uint)]",
      "types=[Target(b'text/uri-list',0,0)]",
      ...(kind === 'mixed' ? ["types.append(Target(b'image/png',0,1))"] : []),
      'entries=(Target*len(types))(*types)',
      'get_type=c.CFUNCTYPE(None,c.c_void_p,c.c_void_p,c.c_uint,c.c_void_p)',
      'clear_type=c.CFUNCTYPE(None,c.c_void_p,c.c_void_p)',
      'def get(clip, selection, info, user):',
      "    if info==1: print('requested-image',flush=True); return",
      "    print('requested-file-list', flush=True)",
      "    uri=(c.c_char_p*2)(os.environ['RHIZOMIND_CLIPBOARD_URI'].encode(),None)",
      '    gtk.gtk_selection_data_set_uris.argtypes=[c.c_void_p,c.POINTER(c.c_char_p)]',
      '    gtk.gtk_selection_data_set_uris(selection,uri)',
      'get_callback=get_type(get)', 'clear_callback=clear_type(lambda clip,user:None)',
      'gtk.gtk_clipboard_set_with_data.argtypes=[c.c_void_p,c.POINTER(Target),c.c_uint,get_type,clear_type,c.c_void_p]',
      'assert gtk.gtk_clipboard_set_with_data(cb,entries,len(types),get_callback,clear_callback,None)',
    ] : [
      'gtk.gtk_clipboard_set_text.argtypes=[c.c_void_p,c.c_char_p,c.c_int]',
      "gtk.gtk_clipboard_set_text(cb,os.environ['RHIZOMIND_CLIPBOARD_TEXT'].encode(),-1)",
    ]),
    "print('ready',flush=True)", 'gtk.gtk_main()',
  ].join('\n');
  const helper = windows
    ? spawn('powershell.exe', ['-NoProfile','-STA','-Command',win], { windowsHide: true, env })
    : spawn('python3', ['-c',linux], { env });
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => { helper.kill(); reject(new Error('буфер не готов')); }, 20_000);
    const fail = (error: unknown) => { clearTimeout(timer); reject(error); };
    const errors: string[] = [];
    helper.stderr?.on('data', (chunk: Buffer) => errors.push(chunk.toString()));
    helper.once('error', fail);
    helper.once('exit', (code) => fail(new Error(`буфер: ${code}, ${errors.join('')}`)));
    let output = '';
    helper.stdout?.on('data', (chunk: Buffer) => {
      output += chunk.toString();
      if (output.includes('ready')) { clearTimeout(timer); resolve(); }
    });
  });
  return helper;
}
