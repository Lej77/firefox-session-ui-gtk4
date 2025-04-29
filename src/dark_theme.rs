#![cfg_attr(not(windows), expect(unused_imports, unused_variables))]

use std::sync::OnceLock;

use glib::object::IsA;
use gtk::{
    gdk, glib,
    prelude::{Cast, NativeExt, WidgetExt},
};
use relm4::gtk;

/// Cached whether the theme is dark or not. Since we don't change the
/// application settings after startup its best to re-use the same value.
static IS_DARK: OnceLock<bool> = OnceLock::new();

pub fn is_dark() -> bool {
    *IS_DARK.get_or_init(|| matches!(dark_light::detect(), Ok(dark_light::Mode::Dark)))
}

/// Need to have initialize GTK.
pub fn set_for_app() {
    #[cfg(windows)]
    {
        // Override default settings specified by gtk, on Windows this is :
        // settings.ini files in /etc/gtk-4.0
        //
        // https://docs.gtk.org/gtk4/class.Settings.html
        let display = gdk::Display::default().expect("GTK display not found");
        gtk::Settings::for_display(&display).set_gtk_application_prefer_dark_theme(is_dark());
    }
}

pub fn set_for_window<W>(window: &W)
where
    W: IsA<gtk::Window>,
{
    #[cfg(windows)]
    {
        let window = W::clone(window).upcast();

        // Can't get surface unless window has been created:
        if window.surface().is_none() {
            #[cfg(debug_assertions)]
            eprintln!("WARNING: window had not been created yet, showing it early");

            window.show();
        }

        set_for_window_surface(&window.surface().expect("Can't get surface for window"));
    }
}
/// Need to manually tell Windows that the native title bar can be dark:
///
/// # References
///
/// - [Support Dark and Light themes in Win32 apps - Windows apps |
///   Microsoft
///   Learn](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/apply-windows-themes)
/// - [c++ - Change the color of the title bar (caption) of a win32
///   application - Stack
///   Overflow](https://stackoverflow.com/questions/39261826/change-the-color-of-the-title-bar-caption-of-a-win32-application)
/// - [Can I use native titlebar in Windows OS - Platform - GNOME
///   Discourse](https://discourse.gnome.org/t/can-i-use-native-titlebar-in-windows-os/10899/3)
/// - [How do I enable and disable the minimize, maximize, and close buttons
///   in my caption bar? - The Old New
///   Thing](https://devblogs.microsoft.com/oldnewthing/20100604-00/?p=13803)
/// - Changes window size to update title bar:
///   <https://stackoverflow.com/questions/74667186/redraw-titlebar-after-setting-dark-mode-in-windows>
/// - <https://stackoverflow.com/questions/57124243/winforms-dark-title-bar-on-windows-10/62811758#62811758>
/// - Handles changes to theme while running:
///   <https://raw.githubusercontent.com/zserge/webview/master/webview.h>
/// - Handles changed to theme:
///   <https://yhetil.org/emacs-bugs/83r1c5tyar.fsf@gnu.org/T/>
pub fn set_for_window_surface(window: &impl IsA<gtk::gdk::Surface>) {
    #[cfg(windows)]
    #[allow(unused_imports)]
    {
        use windows::Win32::{
            Foundation::BOOL,
            Graphics::Dwm::{
                DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE,
            },
            UI::WindowsAndMessaging::{
                GetWindowLongW, GetWindowRect, SetWindowLongW, SetWindowPos, GWL_STYLE,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREPOSITION, SWP_NOZORDER,
                WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
            },
        };

        let is_dark = BOOL::from(is_dark());

        let handle = gdk_win32::Win32Surface::impl_hwnd(window);
        unsafe {
            let mut was_dark = BOOL::default();
            let was_dark_size = std::mem::size_of_val(&was_dark);
            DwmGetWindowAttribute(
                handle,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &mut was_dark as *mut BOOL as *mut _,
                was_dark_size as u32,
            )
            .expect("Failed to get dark theme for window title bar");
            if was_dark == is_dark {
                return; // Title bar already has the right theme!
            }

            DwmSetWindowAttribute(
                handle,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &is_dark as *const BOOL as *const _,
                std::mem::size_of_val(&is_dark) as u32,
            )
            .expect("Failed to set dark theme for window title bar");

            // Enable minimize and maximize buttons (these were broken in
            // earlier GTK versions, but this workaround didn't actually fix
            // the issue since the buttons were immediately disabled by GTK
            // again...):

            // SetWindowLongW(
            //     handle,
            //     GWL_STYLE,
            //     GetWindowLongW(handle, GWL_STYLE)
            //         | ((WS_MINIMIZEBOX | WS_MAXIMIZEBOX).0 as i32),
            // );

            // Redraw the title bar (currently done by slightly resizing the window):
            let mut rect = Default::default();
            GetWindowRect(handle, &mut rect).expect("Failed to get window size");
            SetWindowPos(
                handle,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left - 1,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOREPOSITION | SWP_NOZORDER,
            )
            .expect("Failed to redraw title bar using SetWindowPos");

            SetWindowPos(
                handle,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOREPOSITION | SWP_NOZORDER,
            )
            .expect("Failed to redraw title bar using SetWindowPos");
        }
    }
}
