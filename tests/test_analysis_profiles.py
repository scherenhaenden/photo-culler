"""Regression coverage for persisted analysis-profile preferences."""

import json

import pytest

from photo_culler.analysis.profiles import DEFAULT_PROFILES, AnalysisProfileStore


@pytest.mark.parametrize("payload", [[], {"profiles": {}}, {"profiles": ["not a profile"]}])
def test_profile_store_ignores_valid_json_with_invalid_shape(tmp_path, payload):
    path = tmp_path / "profiles.json"
    path.write_text(json.dumps(payload), encoding="utf-8")

    store = AnalysisProfileStore(path)

    assert {profile["id"] for profile in store.list()} == set(DEFAULT_PROFILES)
