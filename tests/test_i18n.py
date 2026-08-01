from types import SimpleNamespace

import pytest

from photo_culler.web.i18n import LANGUAGES, localize_html, resolve_locale, translate


def request(query=None, cookies=None, accept_language=""):
    return SimpleNamespace(
        query_params=query or {},
        cookies=cookies or {},
        headers={"accept-language": accept_language},
    )


def test_twenty_languages_are_available():
    assert len(LANGUAGES) == 20
    assert {"es", "en", "de", "it", "pt"} <= {language.code for language in LANGUAGES}


@pytest.mark.parametrize(
    ("locale", "expected"),
    [("en", "Library"), ("de", "Bibliothek"), ("it", "Libreria"), ("pt", "Biblioteca"), ("ja", "ライブラリ")],
)
def test_core_navigation_is_translated(locale, expected):
    assert translate("Biblioteca", locale) == expected


def test_locale_resolution_precedence_and_regional_tags():
    assert resolve_locale(request({"lang": "de"}, {"photo_culler_locale": "it"}, "en-US")) == "de"
    assert resolve_locale(request(cookies={"photo_culler_locale": "it"}, accept_language="en-US")) == "it"
    assert resolve_locale(request(accept_language="pt-BR,pt;q=0.9,en;q=0.8")) == "pt"
    assert resolve_locale(request({"lang": "invalid"})) == "es"


def test_html_localization_updates_language_without_mutating_template_structure():
    html = '<html lang="es"><body><span>Biblioteca</span><!-- Active Operator Footer --><script>"Biblioteca"</script></body></html>'
    localized = localize_html(html, "de")
    assert '<html lang="de">' in localized
    assert "<span>Bibliothek</span>" in localized
    assert 'id="language-picker"' not in localized
    assert '<script>"Biblioteca"</script>' in localized


def test_html_localization_translates_markup_after_a_script_without_mutating_the_script():
    html = '<html lang="es"><body><script>const value = "Biblioteca";</script><span>Biblioteca</span></body></html>'
    localized = localize_html(html, "de")

    assert '<script>const value = "Biblioteca";</script>' in localized
    assert "<span>Bibliothek</span>" in localized


def test_html_localization_updates_a_lang_attribute_with_other_attributes():
    localized = localize_html('<html class="app" LANG="en"><body></body></html>', "de")
    assert 'LANG="de"' in localized


def test_html_localization_does_not_restore_protected_blocks_into_matching_content():
    html = (
        '<html lang="es"><body><p>__PHOTO_CULLER_PROTECTED_0__</p>'
        "<script>const protectedBlock = true;</script></body></html>"
    )

    localized = localize_html(html, "de")

    assert localized.count("<script>const protectedBlock = true;</script>") == 1
    assert "<p>__PHOTO_CULLER_PROTECTED_0__</p>" in localized
