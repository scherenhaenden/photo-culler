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


def test_html_localization_updates_language_and_adds_picker():
    html = '<html lang="es"><body><span>Biblioteca</span><!-- Active Operator Footer --><script>"Biblioteca"</script></body></html>'
    localized = localize_html(html, "de")
    assert '<html lang="de">' in localized
    assert "<span>Bibliothek</span>" in localized
    assert 'id="language-picker"' in localized
    assert '<script>"Biblioteca"</script>' in localized
