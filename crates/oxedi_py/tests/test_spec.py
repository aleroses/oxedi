import inspect
import json

import pytest

import oxedi
from conftest import LARGEST, read
from oxedi import Spec, SpecError, Version

PATCH = {"loops": {"ZZ": {"parent": "2100", "trigger": {"segment": "ZZ1"}}}}


def test_the_builtin_spec_loads_and_round_trips_through_json():
    spec = Spec.builtin()
    assert "2100" in spec.loops()
    assert Spec.from_json(spec.to_json()).to_json() == spec.to_json()


def test_a_bad_spec_raises_spec_error_with_the_core_message():
    with pytest.raises(SpecError) as info:
        Spec.from_json('{"name": "x", "loops": {"a": {"trigger": 1}}}')
    assert isinstance(info.value, ValueError)
    assert str(info.value) == (
        "spec: the value at loops.a.trigger must be a JSON object; found a number"
    )


def test_invalid_json_names_the_parser_message():
    with pytest.raises(SpecError) as info:
        Spec.from_json("{")
    assert str(info.value) == (
        "invalid spec JSON: EOF while parsing an object at line 1 column 1"
    )


@pytest.mark.parametrize("as_text", [False, True])
def test_a_patch_adds_a_loop(as_text):
    patch = json.dumps(PATCH) if as_text else PATCH
    patched = Spec.builtin().patch(patch)
    assert "ZZ" in patched.loops()
    assert "ZZ" not in Spec.builtin().loops()


def test_a_patch_that_breaks_the_spec_raises_with_the_inner_error():
    bad = {"loops": {"ZZ": {"parent": "nope", "trigger": {"segment": "ZZ1"}}}}
    with pytest.raises(SpecError) as info:
        Spec.builtin().patch(bad)
    assert str(info.value) == 'applying patch: loop "ZZ" names unknown parent "nope"'
    assert type(info.value).__module__ == "oxedi"


def test_a_patch_must_be_a_dict_or_a_string():
    with pytest.raises(TypeError) as info:
        Spec.builtin().patch(3)
    assert str(info.value) == "Spec.patch takes a dict or a JSON string, not int"


def test_a_dict_that_json_cannot_write_raises_type_error():
    with pytest.raises(TypeError) as info:
        Spec.builtin().patch({"loops": {1}})
    assert str(info.value) == (
        "Spec.patch: the dict cannot be written as JSON: "
        "Object of type set is not JSON serializable"
    )


def test_the_spec_repr():
    assert repr(Spec.builtin()) == "Spec(name='835', loops=8, tables=5)"


COMPAT_TABLES = [
    "rows", "rows_adjustment_groups", "rows_adjustments", "rows_claim_adjustments",
    "rows_claim_amounts", "rows_claim_dates", "rows_claim_entities", "rows_claim_references",
    "rows_claims", "rows_interchanges", "rows_organizations", "rows_payments",
    "rows_provider_adjustments", "rows_references", "rows_remarks", "rows_service_amounts",
    "rows_service_dates",
]


def test_the_edi_835_parser_patch_loads_with_its_tables():
    from oxedi._core import EDI_835_PARSER_PATCH

    spec = Spec.builtin().patch(EDI_835_PARSER_PATCH)
    assert repr(spec) == "Spec(name='edi_835_parser', loops=8, tables=17)"
    result = oxedi.parse(read(LARGEST), spec=spec)
    assert result.tables.keys() == COMPAT_TABLES
    assert (len(result.tables["rows"]), len(result.tables["rows_claims"]), len(result.diagnostics)) == (6192, 1332, 0)


def test_the_builtins_by_version():
    five, four = Spec.builtin(version="5010"), Spec.builtin(version="4010")
    assert five.to_json() == Spec.builtin().to_json()
    assert json.loads(four.to_json())["version"]["values"][0] == "004010X091A1"
    assert json.loads(five.to_json())["version"]["values"] == ["005010X221A1"]


def test_a_version_names_its_release():
    assert str(Version.V5010) == "5010"
    assert Version.V4010.text == "4010"
    assert Version.V5010 != Version.V4010
    assert repr(Version.V5010) == "Version.V5010"
    assert repr(Version.V4010) == "Version.V4010"
    assert len({Version.V5010, Version.V5010, Version.V4010}) == 2
    assert Spec.DEFAULT_VERSION == Version.V5010
    assert Version.V5010 != 0
    assert Version.V5010 != 5010


def test_every_listed_version_is_a_member_of_the_class():
    listed = Version.all()
    assert listed
    assert listed[0] == Spec.DEFAULT_VERSION
    for version in listed:
        assert getattr(Version, repr(version).split(".")[1]) == version


def test_builtin_takes_a_version_or_its_text():
    assert Spec.builtin(Version.V4010).to_json() == Spec.builtin("4010").to_json()
    assert Spec.builtin(Version.V5010).to_json() == Spec.builtin().to_json()
    with pytest.raises(TypeError) as info:
        Spec.builtin(4010)  # type: ignore[arg-type]
    assert str(info.value) == (
        "Spec.builtin(version=4010): version must be an oxedi.Version or its text, not int"
    )


def test_a_version_whose_repr_fails_is_still_named_by_its_type():
    class Broken:
        def __repr__(self) -> str:
            raise RuntimeError("no repr")

    with pytest.raises(TypeError) as info:
        Spec.builtin(Broken())  # type: ignore[arg-type]
    assert str(info.value) == (
        "Spec.builtin(version=an object whose repr failed): version must be an oxedi.Version "
        "or its text, not test_a_version_whose_repr_fails_is_still_named_by_its_type.<locals>.Broken"
    )


def test_builtin_runtime_parameters_are_pinned():
    # The stub test allows the default's runtime text (`...`); the parameters stay pinned here.
    parameters = inspect.signature(Spec.builtin).parameters
    assert list(parameters) == ["version"]
    assert parameters["version"].kind is inspect.Parameter.POSITIONAL_OR_KEYWORD
    assert Spec.builtin().to_json() == Spec.builtin(Spec.DEFAULT_VERSION).to_json()


def test_builtin_without_a_version_is_the_default():
    assert Spec.builtin().to_json() == Spec.builtin("5010").to_json()


def test_an_unknown_builtin_version_names_the_versions_there_are():
    with pytest.raises(ValueError) as info:
        Spec.builtin(version="6020")
    assert str(info.value) == (
        'Spec.builtin(version="6020"): unknown version "6020"; '
        'version must be one of "5010", "4010"'
    )


def lines(diagnostics):
    return [str(d) for d in diagnostics]


@pytest.mark.parametrize(
    "name, version",
    [("edi835_test_davisvision.RMT", "4010"), (LARGEST, "5010")],
)
def test_without_a_spec_the_file_gets_the_builtin_of_its_version(name, version, tmp_path):
    data = read(name)
    chosen = Spec.builtin(version=version)
    expected = lines(oxedi.parse(data, spec=chosen).diagnostics)
    assert lines(oxedi.parse(data).diagnostics) == expected
    path = tmp_path / name
    path.write_bytes(data)
    assert lines(oxedi.parse_file(path).diagnostics) == expected
    streamed = [d for batch in oxedi.stream(data) for d in batch.diagnostics]
    assert lines(streamed) == expected


def test_a_given_spec_is_used_as_given():
    data = read("edi835_test_davisvision.RMT")
    kinds = [d.kind for d in oxedi.parse(data, spec=Spec.builtin()).diagnostics]
    assert kinds.count("CodeNotInList") == 2
    assert "CodeNotInList" not in [d.kind for d in oxedi.parse(data).diagnostics]
    streamed = [d.kind for b in oxedi.stream(data, spec=Spec.builtin()) for d in b.diagnostics]
    assert streamed.count("CodeNotInList") == 2
