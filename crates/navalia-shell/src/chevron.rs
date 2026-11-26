//! The Windows 11 taskbar's "show hidden icons" button, pressed through UI
//! Automation as a screen reader would.
//!
//! Invoking the button opens and closes the hidden-icons flyout exactly as a
//! click on it does. No key is sent, so the flyout never draws the keyboard
//! focus box that Win+B and Space left on its icons.

use windows::{
    Win32::{
        Foundation::HWND,
        System::{
            Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx},
            Variant::VARIANT,
        },
        UI::Accessibility::{
            CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
            TreeScope_Descendants, UIA_AutomationIdPropertyId, UIA_InvokePatternId,
        },
    },
    core::{BSTR, Result},
};

/// The automation id the taskbar gives every button of its notification area
/// (the chevron, the input indicator, network, volume, clock and so on).
const SYSTEM_TRAY_BUTTON: &str = "SystemTrayIcon";

/// Presses the hidden-icons chevron of Explorer's taskbar. Returns false when
/// the button cannot be found or pressed (Windows 10, or no hidden icons), so
/// the caller can fall back to the keyboard.
///
/// Call it off the UI thread: UI Automation waits on Explorer.
pub(crate) fn press() -> bool {
    let Some(tray) = crate::taskbar::explorer_tray() else {
        return false;
    };
    // SAFETY: COM is initialised for this thread (a no-op error when it
    // already is), and every call works on interfaces it returned.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        press_in(tray).unwrap_or(false)
    }
}

unsafe fn press_in(tray: HWND) -> Result<bool> {
    // SAFETY: See `press`.
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let root = automation.ElementFromHandle(tray)?;
        let condition = automation.CreatePropertyCondition(
            UIA_AutomationIdPropertyId,
            &VARIANT::from(BSTR::from(SYSTEM_TRAY_BUTTON)),
        )?;
        let buttons = root.FindAll(TreeScope_Descendants, &condition)?;
        for index in 0..buttons.Length()? {
            let button = buttons.GetElement(index)?;
            if is_chevron(&automation, &button)? {
                let invoke: IUIAutomationInvokePattern =
                    button.GetCurrentPatternAs(UIA_InvokePatternId)?;
                invoke.Invoke()?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// The chevron's label is translated, so it is told apart by its face: a
/// single icon-font glyph. The input indicator next to it, the only other
/// plain button there, shows letters ("ENG").
unsafe fn is_chevron(automation: &IUIAutomation, button: &IUIAutomationElement) -> Result<bool> {
    // SAFETY: See `press`.
    unsafe {
        let walker = automation.RawViewWalker()?;
        let mut child = walker.GetFirstChildElement(button).ok();
        while let Some(element) = child {
            if is_icon_glyph(&element.CurrentName()?.to_string()) {
                return Ok(true);
            }
            child = walker.GetNextSiblingElement(&element).ok();
        }
        Ok(false)
    }
}

/// One character from the private use area, where Segoe Fluent Icons and
/// Segoe MDL2 Assets keep their glyphs.
fn is_icon_glyph(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(c), None) if ('\u{E000}'..='\u{F8FF}').contains(&c)
    )
}

#[cfg(test)]
mod tests {
    use super::is_icon_glyph;

    #[test]
    fn only_a_single_icon_glyph_counts_as_the_chevron_face() {
        assert!(is_icon_glyph("\u{E971}"));
        assert!(!is_icon_glyph("ENG"));
        assert!(!is_icon_glyph(""));
        assert!(!is_icon_glyph("\u{E971}\u{E971}"));
        assert!(!is_icon_glyph("A"));
    }
}
