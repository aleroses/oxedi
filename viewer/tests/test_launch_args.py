"""The Chromium launch arguments: --no-sandbox only when the sandbox image asks for it."""

from __future__ import annotations

import pytest

from conftest import chromium_launch_args

BASE = {"headless": True, "args": ["--lang=en"]}


@pytest.mark.parametrize("environ", [{}, {"OXEDI_CHROMIUM_NO_SANDBOX": "0"}, {"OXEDI_CHROMIUM_NO_SANDBOX": ""}, {"OXEDI_CHROMIUM_NO_SANDBOX": "true"}])
def test_no_flag_unless_the_variable_is_one(environ):
    assert chromium_launch_args(BASE, environ) == BASE


def test_the_flag_is_appended_and_existing_args_are_kept():
    result = chromium_launch_args(BASE, {"OXEDI_CHROMIUM_NO_SANDBOX": "1"})
    assert result == {"headless": True, "args": ["--lang=en", "--no-sandbox"]}
    assert BASE["args"] == ["--lang=en"]


def test_the_flag_is_added_when_there_are_no_args():
    assert chromium_launch_args({"headless": True}, {"OXEDI_CHROMIUM_NO_SANDBOX": "1"})["args"] == ["--no-sandbox"]
