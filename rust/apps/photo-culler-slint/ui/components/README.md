# Shared Slint components

The semantic palette follows the dark reference at 3701bed: canvas #0F1012, card #111111, hover #1C1D21, sidebar #050505, orange active #FF6B35, and reference foreground #E3E2E4. Best/Keep/Alternate/Reject/Unrated remain separate semantic roles so web Keep/Reject can preserve their blue/orange badge colors while native uses teal/red.

ActionButton and SidebarItem have visible keyboard focus and tab/Enter/Space behavior. StatusPanel distinguishes loading, empty and error and exposes retry only for an error. The gallery shell uses these tokens, but keeps the web card grid separate from the native list/preview.

Run source contract checks:

    python3 rust/apps/photo-culler-slint/ui/components/test_components.py

Paired renderer captures for the egui and legacy web reference still require UX01 collection at 3701bed and UX04 side-by-side review. No screenshot files are included here.
