import re
from importlib import metadata

import oxedi


def test_the_base_package_requires_nothing():
    requirements = metadata.requires("oxedi") or []
    assert requirements and all("extra ==" in r for r in requirements)


def test_the_extras_are_the_documented_ones():
    assert sorted(metadata.metadata("oxedi").get_all("Provides-Extra")) == [
        "edi-835-parser", "pandas", "polars", "pyx12",
    ]


PEP_440 = re.compile(
    r"^([1-9][0-9]*!)?(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*))*"
    r"((a|b|rc)(0|[1-9][0-9]*))?(\.post(0|[1-9][0-9]*))?(\.dev(0|[1-9][0-9]*))?"
    r"(\+[a-z0-9]+(\.[a-z0-9]+)*)?$"
)


def test_the_module_version_is_the_distribution_version():
    version = metadata.version("oxedi")
    assert oxedi.__version__ == version
    try:
        from packaging.version import Version
    except ImportError:
        assert PEP_440.match(version), version
    else:
        assert str(Version(version)) == version
