#![windows_subsystem = "windows"]

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Map, Value};
use std::ffi::c_void;
use std::path::Path;
use std::ptr::null;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ==================== 主题色 ====================
type Rgb = (u8, u8, u8);

const C_BG: Rgb = (23, 24, 27);
const C_CARD: Rgb = (37, 39, 44);
const C_TEXT: Rgb = (231, 233, 236);
const C_DIM: Rgb = (139, 144, 153);
const C_PRIMARY: Rgb = (79, 142, 247);
const C_PRIMARY_DOWN: Rgb = (54, 112, 210);
const C_SECOND: Rgb = (44, 47, 53);
const C_SECOND_DOWN: Rgb = (58, 62, 70);
const C_OK: Rgb = (86, 214, 138);
const C_ERR: Rgb = (247, 118, 118);
const C_LOG: Rgb = (168, 176, 188);

fn rgb(c: Rgb) -> u32 {
    (c.0 as u32) | ((c.1 as u32) << 8) | ((c.2 as u32) << 16)
}

// ==================== 自声明 Win32 结构 ====================

#[repr(C)]
struct OpenFileNameW {
    struct_size: u32,
    hwnd_owner: HWND,
    hinstance: isize,
    filter: *const u16,
    custom_filter: *mut u16,
    max_cust_filter: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_dir: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    def_ext: *const u16,
    cust_data: LPARAM,
    hook: *mut c_void,
    template_name: *const u16,
    reserved: *mut c_void,
    reserved2: u32,
    flags_ex: u32,
}

#[repr(C)]
struct DrawItemStruct {
    ctl_type: u32,
    ctl_id: u32,
    item_id: u32,
    item_action: u32,
    item_state: u32,
    hwnd_item: HWND,
    hdc: HDC,
    rc_item: RECT,
    item_data: usize,
}

const OFN_HIDEREADONLY: u32 = 0x0000_0004;
const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;

const WM_DROPFILES: u32 = 0x0233;
const WM_CTLCOLOREDIT: u32 = 0x0133;
const WM_CTLCOLORSTATIC: u32 = 0x0138;
const WM_DRAWITEM: u32 = 0x002B;
const ODS_SELECTED: u32 = 0x0001;
const EM_SETMARGINS: u32 = 0x00D3;
const EC_LEFTMARGIN: u32 = 0x0001;
const EC_RIGHTMARGIN: u32 = 0x0002;

#[link(name = "comdlg32")]
extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileNameW) -> i32;
    fn GetSaveFileNameW(ofn: *mut OpenFileNameW) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn DragAcceptFiles(hwnd: HWND, f: i32);
    fn DragQueryFileW(hdrop: isize, ifile: u32, lpszfile: *mut u16, cch: u32) -> u32;
    fn DragFinish(hdrop: isize);
}

#[link(name = "user32")]
extern "system" {
    fn SetProcessDPIAware() -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(lpfilename: *const u16) -> isize;
    fn GetProcAddress(hmodule: isize, lpprocname: *const u8) -> *const c_void;
}

/// 让 EDIT 的滚动条使用系统暗色主题
unsafe fn set_dark_scrollbar(hwnd: HWND) {
    let lib = LoadLibraryW(wide("uxtheme.dll").as_ptr());
    if lib == 0 {
        return;
    }
    let proc = GetProcAddress(lib, b"SetWindowTheme\0".as_ptr());
    if proc.is_null() {
        return;
    }
    let f: extern "system" fn(HWND, *const u16, *const u16) -> i32 = std::mem::transmute(proc);
    let theme = wide("DarkMode_Explorer");
    f(hwnd, theme.as_ptr(), null());
}

/// 加载程序同目录下的 app.ico 作为窗口/任务栏图标（文件不存在则不设置）
unsafe fn apply_icon(hwnd: HWND) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    let ico = match exe.parent() {
        Some(d) => d.join("app.ico"),
        None => return,
    };
    if !ico.exists() {
        return;
    }
    let path = wide(&ico.display().to_string());
    const IMAGE_ICON: u32 = 1;
    const LR_LOADFROMFILE: u32 = 0x0000_0010;
    let big = LoadImageW(0, path.as_ptr(), IMAGE_ICON, px(32), px(32), LR_LOADFROMFILE);
    let small = LoadImageW(0, path.as_ptr(), IMAGE_ICON, px(16), px(16), LR_LOADFROMFILE);
    if big != 0 {
        SendMessageW(hwnd, WM_SETICON, 1, big as LPARAM); // ICON_BIG
    }
    if small != 0 {
        SendMessageW(hwnd, WM_SETICON, 0, small as LPARAM); // ICON_SMALL
    }
}

/// 让系统标题栏也走深色（dwmapi 动态加载，避免链接期缺 libdwmapi.a）
unsafe fn enable_dark_titlebar(hwnd: HWND) {
    let name = wide("dwmapi.dll");
    let lib = LoadLibraryW(name.as_ptr());
    if lib == 0 {
        return;
    }
    let proc = GetProcAddress(lib, b"DwmSetWindowAttribute\0".as_ptr());
    if proc.is_null() {
        return;
    }
    let f: extern "system" fn(HWND, u32, *const c_void, u32) -> i32 = std::mem::transmute(proc);
    let dark: i32 = 1;
    f(hwnd, 20, &dark as *const i32 as *const c_void, 4);
    f(hwnd, 19, &dark as *const i32 as *const c_void, 4);
}

// ==================== 控件 ID ====================
const ID_INPUT: usize = 1001;
const ID_OUTPUT: usize = 1002;
const ID_BROWSE: usize = 1003;
const ID_SAVE: usize = 1004;
const ID_CONVERT: usize = 1005;
const ID_STATUS: usize = 1006;
const ID_LOG: usize = 1007;
const ID_IMPORT: usize = 1008;

/// 拖放窗口把选中路径回传给主窗口
const WM_DROP_PATH: u32 = 0x0400 + 10;

const ID_TITLE: usize = 2001;
const ID_SUBTITLE: usize = 2002;
const ID_LBL_IN: usize = 2003;
const ID_LBL_OUT: usize = 2004;
const ID_LBL_LOG: usize = 2005;

// ==================== 全局状态 ====================
static mut SCALE: f32 = 1.0;
static mut STATUS_KIND: i32 = 1; // 0=普通 1=成功(绿) 2=失败(红)；初始"就绪"用绿色
static mut BRUSH_BG: isize = 0;

fn px(v: i32) -> i32 {
    unsafe { (v as f32 * SCALE).round() as i32 }
}

struct Ui {
    input: HWND,
    output: HWND,
    status: HWND,
    log: HWND,
    font_ui: HFONT,
    font_small: HFONT,
    font_title: HFONT,
    font_bold: HFONT,
    brush_card: HBRUSH,
}

static INIT_PATHS: OnceLock<(String, String)> = OnceLock::new();

// ==================== 转换核心 ====================

fn jwt_exp(token: &str) -> Result<i64, String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "access_token/refresh_token 不是合法 JWT（期望 3 段，实际 {} 段）",
            parts.len()
        ));
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| format!("JWT payload base64url 解码失败: {e}"))?;
    let payload: Value =
        serde_json::from_slice(&decoded).map_err(|e| format!("JWT payload 不是 JSON: {e}"))?;
    payload
        .get("exp")
        .and_then(Value::as_i64)
        .ok_or_else(|| "JWT payload 缺少 exp 字段".to_string())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn local_date_ymd() -> (i64, u32, u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 8 * 3600;
    let z = secs.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub struct Outcome {
    pub input: String,
    pub output: String,
    pub access_exp: i64,
    pub refresh_exp: i64,
    pub created_at: u128,
}

pub fn convert_file(
    input: &str,
    output: &str,
    created_at_override: Option<u128>,
) -> Result<Outcome, String> {
    let raw = std::fs::read_to_string(input).map_err(|e| format!("读取源文件失败: {e}"))?;
    let parsed: Value =
        serde_json::from_str(&raw).map_err(|e| format!("源文件不是合法 JSON: {e}"))?;

    let acct = if parsed.is_array() {
        parsed
            .get(0)
            .cloned()
            .ok_or_else(|| "源文件是空数组".to_string())?
    } else {
        parsed
    };

    let get = |k: &str| -> Result<String, String> {
        acct.get(k)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("缺少字符串字段 {k}，这不像是账号 JSON"))
    };

    let id = get("id")?;
    let uid = get("uid")?;
    let nickname = get("nickname")?;
    let email = get("email")?;
    let access_token = get("access_token")?;
    let refresh_token = get("refresh_token")?;
    let token_type = get("token_type")?;

    let access_exp = jwt_exp(&access_token)?;
    let refresh_exp = jwt_exp(&refresh_token)?;
    let created_at = created_at_override.unwrap_or_else(now_millis);

    let mut entry = Map::new();
    entry.insert("id".into(), json!(id));
    entry.insert("uid".into(), json!(uid));
    entry.insert("nickname".into(), json!(nickname));
    entry.insert("email".into(), Value::Null);
    entry.insert("variant".into(), json!("cn"));
    entry.insert("enterpriseName".into(), Value::Null);
    entry.insert("enterpriseId".into(), json!(""));
    entry.insert("access_token".into(), json!(access_token));
    entry.insert("refresh_token".into(), json!(refresh_token));
    entry.insert("token_type".into(), json!(token_type));
    entry.insert("domain".into(), json!("www.codebuddy.cn"));
    entry.insert("expiresAt".into(), json!(access_exp * 1000));
    entry.insert("refreshExpiresAt".into(), json!(refresh_exp * 1000));
    entry.insert(
        "auth_raw".into(),
        json!({
            "account": { "email": email, "uid": uid, "nickname": nickname },
            "auth": acct
        }),
    );
    entry.insert(
        "profile_raw".into(),
        json!({ "email": email, "uid": uid, "nickname": nickname }),
    );
    entry.insert("createdAt".into(), json!(created_at));

    let doc = json!(vec![Value::Object(entry)]);
    let text = serde_json::to_string_pretty(&doc).map_err(|e| format!("序列化失败: {e}"))?;

    if let Some(parent) = Path::new(output).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    std::fs::write(output, text).map_err(|e| format!("写入输出失败: {e}"))?;

    Ok(Outcome {
        input: input.to_string(),
        output: output.to_string(),
        access_exp,
        refresh_exp,
        created_at,
    })
}

fn derive_output(input: &str) -> String {
    let (y, m, d) = local_date_ymd();
    let name = format!("wb-switch-accounts-{y:04}-{m:02}-{d:02}.json");
    let p = Path::new(input);
    match p.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name).display().to_string(),
        _ => name,
    }
}

/// 目标文件已存在时追加 (1) (2)…，避免覆盖
fn unique_output(path: &str) -> String {
    if !Path::new(path).exists() {
        return path.to_string();
    }
    let p = Path::new(path);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let dir = p.parent();
    for i in 1..10000u32 {
        let name = if ext.is_empty() {
            format!("{stem}({i})")
        } else {
            format!("{stem}({i}).{ext}")
        };
        let cand = match dir {
            Some(d) if !d.as_os_str().is_empty() => d.join(&name).display().to_string(),
            _ => name,
        };
        if !Path::new(&cand).exists() {
            return cand;
        }
    }
    path.to_string()
}

fn detect_initial() -> (String, String) {
    let mut input = String::new();
    if let Ok(rd) = std::fs::read_dir(".") {
        let mut hits: Vec<std::path::PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .map(|x| x.eq_ignore_ascii_case("json"))
                    .unwrap_or(false)
                    && p.file_name()
                        .map(|n| n.to_string_lossy().starts_with("账号"))
                        .unwrap_or(false)
            })
            .collect();
        hits.sort();
        if let Some(p) = hits.first() {
            input = std::env::current_dir()
                .ok()
                .and_then(|d| p.file_name().map(|n| d.join(n)))
                .map(|q| q.display().to_string())
                .unwrap_or_else(|| p.display().to_string());
        }
    }
    let output = derive_output(if input.is_empty() { "out.json" } else { &input });
    (input, output)
}

// ==================== Win32 小工具 ====================

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn get_text(h: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(h);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

fn set_text(h: HWND, s: &str) {
    unsafe {
        let w = wide(s);
        SetWindowTextW(h, w.as_ptr());
        // 光标归零：长路径从开头显示，而不是滚动到结尾
        SendMessageW(h, 0x00B1, 0, 0); // EM_SETSEL
    }
}

fn append_log(h: HWND, line: &str) {
    let mut cur = get_text(h);
    if !cur.is_empty() {
        cur.push_str("\r\n");
    }
    cur.push_str(line);
    let lines: Vec<&str> = cur.split("\r\n").collect();
    if lines.len() > 200 {
        let keep = lines[lines.len() - 200..].join("\r\n");
        set_text(h, &keep);
    } else {
        set_text(h, &cur);
    }
}

fn u16z_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn pick_open(hwnd: HWND) -> Option<String> {
    let mut buf = vec![0u16; 2048];
    let filter: Vec<u16> = "JSON 文件\0*.json\0所有文件\0*.*\0\0".encode_utf16().collect();
    let title = wide("选择账号 JSON 文件");
    let mut ofn: OpenFileNameW = unsafe { std::mem::zeroed() };
    ofn.struct_size = std::mem::size_of::<OpenFileNameW>() as u32;
    ofn.hwnd_owner = hwnd;
    ofn.filter = filter.as_ptr();
    ofn.file = buf.as_mut_ptr();
    ofn.max_file = buf.len() as u32;
    ofn.title = title.as_ptr();
    ofn.flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_HIDEREADONLY;
    let ok = unsafe { GetOpenFileNameW(&mut ofn) };
    if ok == 0 {
        None
    } else {
        Some(u16z_to_string(&buf))
    }
}

fn pick_save(hwnd: HWND) -> Option<String> {
    let mut buf = vec![0u16; 2048];
    for (i, c) in wide("wb-switch-accounts.json").iter().enumerate() {
        if i < buf.len() {
            buf[i] = *c;
        }
    }
    let filter: Vec<u16> = "JSON 文件\0*.json\0所有文件\0*.*\0\0".encode_utf16().collect();
    let title = wide("保存为 wb-switch-accounts 备份");
    let mut ofn: OpenFileNameW = unsafe { std::mem::zeroed() };
    ofn.struct_size = std::mem::size_of::<OpenFileNameW>() as u32;
    ofn.hwnd_owner = hwnd;
    ofn.filter = filter.as_ptr();
    ofn.file = buf.as_mut_ptr();
    ofn.max_file = buf.len() as u32;
    ofn.title = title.as_ptr();
    ofn.flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_HIDEREADONLY;
    let ok = unsafe { GetSaveFileNameW(&mut ofn) };
    if ok == 0 {
        None
    } else {
        Some(u16z_to_string(&buf))
    }
}

fn msgbox(hwnd: HWND, text: &str, title: &str, flags: u32) {
    unsafe {
        let t = wide(text);
        let c = wide(title);
        MessageBoxW(hwnd, t.as_ptr(), c.as_ptr(), flags);
    }
}

/// 圆角按钮自绘
unsafe fn draw_item(dis: &DrawItemStruct) {
    let pressed = dis.item_state & ODS_SELECTED != 0;
    let is_primary = dis.ctl_id as usize == ID_CONVERT;
    let (bg, fg, radius) = if is_primary {
        (
            if pressed { C_PRIMARY_DOWN } else { C_PRIMARY },
            (255, 255, 255),
            px(11),
        )
    } else {
        (
            if pressed { C_SECOND_DOWN } else { C_SECOND },
            C_TEXT,
            px(8),
        )
    };

    let brush = CreateSolidBrush(rgb(bg));
    let old_brush = SelectObject(dis.hdc, brush as HGDIOBJ);
    let null_pen = GetStockObject(NULL_PEN);
    let old_pen = SelectObject(dis.hdc, null_pen);
    let rc = dis.rc_item;
    // 先用窗口背景色铺满整个按钮矩形，抹掉圆角外露出的系统白底
    let bg_brush = CreateSolidBrush(rgb(C_BG));
    FillRect(dis.hdc, &rc, bg_brush);
    DeleteObject(bg_brush as HGDIOBJ);
    RoundRect(
        dis.hdc,
        rc.left,
        rc.top,
        rc.right,
        rc.bottom,
        radius * 2,
        radius * 2,
    );
    SelectObject(dis.hdc, old_brush);
    SelectObject(dis.hdc, old_pen);
    DeleteObject(brush as HGDIOBJ);

    let mut buf = [0u16; 128];
    let n = GetWindowTextW(dis.hwnd_item, buf.as_mut_ptr(), 128);
    if n > 0 {
        SetBkMode(dis.hdc, TRANSPARENT as i32);
        SetTextColor(dis.hdc, rgb(fg));
        let parent = GetParent(dis.hwnd_item);
        let ptr = GetWindowLongPtrW(parent, GWLP_USERDATA) as *const Ui;
        if !ptr.is_null() {
            let font = if is_primary {
                (*ptr).font_bold
            } else {
                (*ptr).font_ui
            };
            SelectObject(dis.hdc, font as HGDIOBJ);
        }
        let mut r = dis.rc_item;
        if pressed {
            r.top += px(1);
        }
        DrawTextW(
            dis.hdc,
            buf.as_ptr(),
            n,
            &mut r,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }
}

// ==================== 拖放导入窗口 ====================

unsafe extern "system" fn drop_wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            DragAcceptFiles(hwnd, 1);
            enable_dark_titlebar(hwnd);
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc);

            // 虚线拖放区
            let pen = CreatePen(PS_DASH as i32, 1, rgb(C_DIM));
            let old_pen = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            RoundRect(
                hdc,
                rc.left + px(18),
                rc.top + px(18),
                rc.right - px(18),
                rc.bottom - px(18),
                px(12) * 2,
                px(12) * 2,
            );
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(pen as HGDIOBJ);

            // 借用主窗口字体
            let owner = GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) as HWND;
            let ui_ptr = GetWindowLongPtrW(owner, GWLP_USERDATA) as *const Ui;

            SetBkMode(hdc, TRANSPARENT as i32);
            if !ui_ptr.is_null() {
                SelectObject(hdc, (*ui_ptr).font_ui as HGDIOBJ);
            }
            SetTextColor(hdc, rgb(C_TEXT));
            let text = wide("把 JSON 文件拖到这里");
            let mut tr = rc;
            tr.top -= px(10);
            DrawTextW(
                hdc,
                text.as_ptr(),
                -1,
                &mut tr,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );

            if !ui_ptr.is_null() {
                SelectObject(hdc, (*ui_ptr).font_small as HGDIOBJ);
            }
            SetTextColor(hdc, rgb(C_DIM));
            let hint = wide("松手自动填入，或点击此处选择");
            let mut tr2 = rc;
            tr2.top += px(28);
            DrawTextW(
                hdc,
                hint.as_ptr(),
                -1,
                &mut tr2,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );

            EndPaint(hwnd, &ps);
            0
        }
        WM_LBUTTONUP => {
            // 兜底：不想拖也能点一下选文件
            if let Some(path) = pick_open(hwnd) {
                let owner = GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) as HWND;
                if owner != 0 {
                    let boxed = Box::new(path);
                    SendMessageW(owner, WM_DROP_PATH, 0, Box::into_raw(boxed) as LPARAM);
                }
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DROPFILES => {
            let hdrop = w as isize;
            let mut buf = vec![0u16; 2048];
            let n = DragQueryFileW(hdrop, 0, buf.as_mut_ptr(), buf.len() as u32);
            DragFinish(hdrop);
            if n > 0 {
                let path = u16z_to_string(&buf);
                if path.to_ascii_lowercase().ends_with(".json") {
                    let owner = GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) as HWND;
                    if owner != 0 {
                        let boxed = Box::new(path);
                        SendMessageW(owner, WM_DROP_PATH, 0, Box::into_raw(boxed) as LPARAM);
                    }
                    DestroyWindow(hwnd);
                } else {
                    let t = wide("只接受 .json 文件");
                    let c = wide("提示");
                    MessageBoxW(hwnd, t.as_ptr(), c.as_ptr(), MB_OK | MB_ICONWARNING);
                }
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

unsafe fn open_drop_window(owner: HWND) {
    let hinst = GetModuleHandleW(null());
    let class_name = wide("WBDropWindow");
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(drop_wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: 0,
        hCursor: LoadCursorW(0, IDC_ARROW),
        hbrBackground: BRUSH_BG as HBRUSH,
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
    };
    RegisterClassW(&wc);

    let mut pr: RECT = std::mem::zeroed();
    GetWindowRect(owner, &mut pr);
    let ww = px(460);
    let wh = px(250);
    let x = pr.left + ((pr.right - pr.left) - ww) / 2;
    let y = pr.top + ((pr.bottom - pr.top) - wh) / 2;

    let title = wide("导入 JSON 文件");
    let h = CreateWindowExW(
        0x0000_0080, // WS_EX_TOOLWINDOW：不出现在任务栏
        class_name.as_ptr(),
        title.as_ptr(),
        WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
        x,
        y,
        ww,
        wh,
        owner,
        0,
        hinst,
        null(),
    );
    if h != 0 {
        ShowWindow(h, SW_SHOW);
        UpdateWindow(h);
        SetForegroundWindow(h);
    }
}

// ==================== 界面构建 ====================

const WM_APP_NOTIFY: u32 = 0x0400 + 1;

unsafe fn create_controls(hwnd: HWND) -> Ui {
    let hinst = GetModuleHandleW(null());
    let (init_in, init_out) = INIT_PATHS.get().cloned().unwrap_or_default();

    let face = wide("Microsoft YaHei");
    let mk_font = |h: i32, weight: i32| -> HFONT {
        CreateFontW(
            px(h),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            face.as_ptr(),
        )
    };
    let font_ui = mk_font(-17, FW_NORMAL as i32);
    let font_small = mk_font(-15, FW_NORMAL as i32);
    let font_title = mk_font(-30, FW_BOLD as i32);
    let font_bold = mk_font(-18, 600);

    let brush_card = CreateSolidBrush(rgb(C_CARD));

    let mk = |class: &str,
              text: &str,
              style: u32,
              x: i32,
              y: i32,
              w: i32,
              h: i32,
              id: usize,
              font: HFONT|
     -> HWND {
        let c = wide(class);
        let t = wide(text);
        let hc = CreateWindowExW(
            0,
            c.as_ptr(),
            t.as_ptr(),
            style,
            px(x),
            px(y),
            px(w),
            px(h),
            hwnd,
            id as isize,
            hinst,
            null(),
        );
        SendMessageW(hc, WM_SETFONT, font as WPARAM, 1);
        hc
    };

    // 标题 + 副标题
    mk(
        "STATIC",
        "WBTransfer",
        WS_CHILD | WS_VISIBLE,
        32,
        26,
        700,
        42,
        ID_TITLE,
        font_title,
    );
    mk(
        "STATIC",
        "单账号 JSON",
        WS_CHILD | WS_VISIBLE,
        32,
        74,
        836,
        22,
        ID_SUBTITLE,
        font_small,
    );

    // 源文件
    mk(
        "STATIC",
        "源文件",
        WS_CHILD | WS_VISIBLE,
        32,
        118,
        200,
        20,
        ID_LBL_IN,
        font_small,
    );
    let input = mk(
        "EDIT",
        &init_in,
        WS_CHILD | WS_VISIBLE | (ES_LEFT as u32) | (ES_AUTOHSCROLL as u32),
        32,
        144,
        644,
        38,
        ID_INPUT,
        font_ui,
    );
    SendMessageW(
        input,
        EM_SETMARGINS,
        (EC_LEFTMARGIN | EC_RIGHTMARGIN) as WPARAM,
        ((px(10) << 16) | px(10)) as LPARAM,
    );
    mk(
        "BUTTON",
        "浏览…",
        WS_CHILD | WS_VISIBLE | (BS_OWNERDRAW as u32),
        688,
        144,
        84,
        38,
        ID_BROWSE,
        font_ui,
    );
    mk(
        "BUTTON",
        "导入",
        WS_CHILD | WS_VISIBLE | (BS_OWNERDRAW as u32),
        784,
        144,
        84,
        38,
        ID_IMPORT,
        font_ui,
    );

    // 输出
    mk(
        "STATIC",
        "输出到",
        WS_CHILD | WS_VISIBLE,
        32,
        202,
        200,
        20,
        ID_LBL_OUT,
        font_small,
    );
    let output = mk(
        "EDIT",
        &init_out,
        WS_CHILD | WS_VISIBLE | (ES_LEFT as u32) | (ES_AUTOHSCROLL as u32),
        32,
        228,
        740,
        38,
        ID_OUTPUT,
        font_ui,
    );
    SendMessageW(
        output,
        EM_SETMARGINS,
        (EC_LEFTMARGIN | EC_RIGHTMARGIN) as WPARAM,
        ((px(10) << 16) | px(10)) as LPARAM,
    );
    mk(
        "BUTTON",
        "另存为…",
        WS_CHILD | WS_VISIBLE | (BS_OWNERDRAW as u32),
        784,
        228,
        84,
        38,
        ID_SAVE,
        font_ui,
    );

    // 主按钮
    mk(
        "BUTTON",
        "开始转换",
        WS_CHILD | WS_VISIBLE | (BS_OWNERDRAW as u32),
        32,
        296,
        168,
        46,
        ID_CONVERT,
        font_bold,
    );

    // 状态
    let status = mk(
        "STATIC",
        "就绪",
        WS_CHILD | WS_VISIBLE,
        32,
        366,
        836,
        22,
        ID_STATUS,
        font_small,
    );

    // 日志
    mk(
        "STATIC",
        "日志",
        WS_CHILD | WS_VISIBLE,
        32,
        408,
        200,
        18,
        ID_LBL_LOG,
        font_small,
    );
    let log = mk(
        "EDIT",
        "",
        WS_CHILD | WS_VISIBLE | WS_VSCROLL
            | (ES_MULTILINE as u32)
            | (ES_READONLY as u32)
            | (ES_AUTOVSCROLL as u32)
            | (ES_LEFT as u32),
        32,
        432,
        836,
        130,
        ID_LOG,
        font_small,
    );
    SendMessageW(
        log,
        EM_SETMARGINS,
        (EC_LEFTMARGIN | EC_RIGHTMARGIN) as WPARAM,
        ((px(8) << 16) | px(8)) as LPARAM,
    );
    set_dark_scrollbar(log);

    Ui {
        input,
        output,
        status,
        log,
        font_ui,
        font_small,
        font_title,
        font_bold,
        brush_card,
    }
}

unsafe fn set_status(ui: &Ui, kind: i32, text: &str) {
    STATUS_KIND = kind;
    set_text(ui.status, text);
    InvalidateRect(ui.status, null(), 1);
}

unsafe fn do_convert(hwnd: HWND, ui: &Ui) {
    let input = get_text(ui.input);
    if input.trim().is_empty() {
        set_status(ui, 2, "请先选择源文件。");
        return;
    }
    let mut out = get_text(ui.output).trim().to_string();
    if out.is_empty() {
        out = derive_output(&input);
    }
    // 同名文件已存在则自动加序号，绝不覆盖已有文件
    out = unique_output(&out);
    set_text(ui.output, &out);

    append_log(ui.log, &format!("→ {input}  →  {out}"));
    match convert_file(&input, &out, None) {
        Ok(o) => {
            append_log(
                ui.log,
                &format!("access  exp {}  →  expiresAt {}", o.access_exp, o.access_exp * 1000),
            );
            append_log(
                ui.log,
                &format!("refresh exp {}  →  refreshExpiresAt {}", o.refresh_exp, o.refresh_exp * 1000),
            );
            append_log(ui.log, &format!("createdAt {}", o.created_at));
            append_log(ui.log, "完成。");
            set_status(ui, 1, &format!("✓  已生成  {}", o.output));
            let _ = hwnd;
        }
        Err(e) => {
            append_log(ui.log, &format!("× {e}"));
            set_status(ui, 2, &format!("×  {e}"));
        }
    }
}

unsafe fn on_command(hwnd: HWND, id: usize, ui: &Ui) {
    match id {
        ID_BROWSE => {
            if let Some(p) = pick_open(hwnd) {
                set_text(ui.input, &p);
                set_text(ui.output, &derive_output(&p));
                append_log(ui.log, &format!("已选择：{p}"));
                set_status(ui, 0, "已载入源文件，点「开始转换」。");
            }
        }
        ID_SAVE => {
            if let Some(p) = pick_save(hwnd) {
                set_text(ui.output, &p);
            }
        }
        ID_CONVERT => do_convert(hwnd, ui),
        ID_IMPORT => open_drop_window(hwnd),
        _ => {}
    }
    let _ = WM_APP_NOTIFY;
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let ui = Box::new(create_controls(hwnd));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(ui) as isize);
            0
        }
        WM_COMMAND => {
            let id = (w & 0xFFFF) as usize;
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Ui;
            if !ptr.is_null() {
                on_command(hwnd, id, &*ptr);
            }
            0
        }
        WM_DRAWITEM => {
            let dis = &*(l as *const DrawItemStruct);
            draw_item(dis);
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
            let hdc = w as HDC;
            let ctl = l as HWND;
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Ui;
            let (is_log, is_status) = if ptr.is_null() {
                (false, false)
            } else {
                (ctl == (*ptr).log, ctl == (*ptr).status)
            };

            if msg == WM_CTLCOLOREDIT || is_log {
                SetTextColor(hdc, rgb(if is_log { C_LOG } else { C_TEXT }));
                SetBkColor(hdc, rgb(C_CARD));
                if !ptr.is_null() {
                    return (*ptr).brush_card as LRESULT;
                }
                return GetStockObject(WHITE_BRUSH) as LRESULT;
            }

            SetBkMode(hdc, TRANSPARENT as i32);
            SetBkColor(hdc, rgb(C_BG));
            let id = GetDlgCtrlID(ctl) as usize;
            let color = if is_status {
                match STATUS_KIND {
                    1 => C_OK,
                    2 => C_ERR,
                    _ => C_DIM,
                }
            } else if id == ID_TITLE {
                C_TEXT
            } else if id == ID_SUBTITLE {
                C_DIM
            } else {
                C_DIM
            };
            SetTextColor(hdc, rgb(color));
            BRUSH_BG as LRESULT
        }
        WM_DROPFILES => {
            let hdrop = w as isize;
            let mut buf = vec![0u16; 2048];
            let n = DragQueryFileW(hdrop, 0, buf.as_mut_ptr(), buf.len() as u32);
            DragFinish(hdrop);
            if n > 0 {
                let path = u16z_to_string(&buf);
                if path.to_ascii_lowercase().ends_with(".json") {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Ui;
                    if !ptr.is_null() {
                        let ui = &*ptr;
                        set_text(ui.input, &path);
                        set_text(ui.output, &derive_output(&path));
                        append_log(ui.log, &format!("[拖入] {path}"));
                        set_status(ui, 0, "已载入拖入的文件，点「开始转换」。");
                    }
                }
            }
            0
        }
        WM_DROP_PATH => {
            let ptr = l as *mut String;
            if !ptr.is_null() {
                let path = *Box::from_raw(ptr);
                let ui_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Ui;
                if !ui_ptr.is_null() {
                    let ui = &*ui_ptr;
                    set_text(ui.input, &path);
                    set_text(ui.output, &unique_output(&derive_output(&path)));
                    append_log(ui.log, &format!("[导入] {path}"));
                    set_status(ui, 1, "已导入源文件，点「开始转换」。");
                }
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ui;
            if !ptr.is_null() {
                let boxed = Box::from_raw(ptr);
                DeleteObject(boxed.font_ui as HGDIOBJ);
                DeleteObject(boxed.font_small as HGDIOBJ);
                DeleteObject(boxed.font_title as HGDIOBJ);
                DeleteObject(boxed.font_bold as HGDIOBJ);
                DeleteObject(boxed.brush_card as HGDIOBJ);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn run_gui() {
    let (i, o) = detect_initial();
    let _ = INIT_PATHS.set((i, o));

    unsafe {
        SetProcessDPIAware();
        let dc = GetDC(0);
        let dpi = GetDeviceCaps(dc, LOGPIXELSX as i32);
        ReleaseDC(0, dc);
        SCALE = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };

        let hinst = GetModuleHandleW(null());
        BRUSH_BG = CreateSolidBrush(rgb(C_BG)) as isize;

        let class_name = wide("WBAccountConverterWindow");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: 0,
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: BRUSH_BG as HBRUSH,
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            msgbox(0, "窗口类注册失败", "错误", MB_OK | MB_ICONERROR);
            return;
        }

        let ww = px(900);
        let wh = px(600);
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let x = (sw - ww) / 2;
        let y = (sh - wh) / 2;

        let title = wide("WBTransfer");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            (WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX & !WS_THICKFRAME) | WS_CLIPCHILDREN,
            x,
            y,
            ww,
            wh,
            0,
            0,
            hinst,
            null(),
        );
        if hwnd == 0 {
            msgbox(0, "窗口创建失败", "错误", MB_OK | MB_ICONERROR);
            return;
        }

        DragAcceptFiles(hwnd, 1);
        apply_icon(hwnd);
        enable_dark_titlebar(hwnd);
        // 对整个窗口应用暗色主题，子控件（含 EDIT 滚动条）统一走 DarkMode
        set_dark_scrollbar(hwnd);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        // 再应用一次：调用时窗口已可见，主题才彻底生效
        set_dark_scrollbar(hwnd);
        let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Ui;
        if !p.is_null() {
            set_dark_scrollbar((*p).log);
        }
        // 创建期间 GWLP_USERDATA 尚未写入，STATIC 会用默认白刷兜底；
        // 这里强制重绘所有子控件，让 WM_CTLCOLOR* 走正确的深色分支。
        RedrawWindow(
            hwnd,
            std::ptr::null::<RECT>(),
            0,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
        );

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn main() {
    run_gui();
}
