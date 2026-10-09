import re

from lariksa.bridge import EXPORT_FILTERS, IMPORT_FILTER, _safe_filename

# Copied from pywebview 6.2 webview/util.py parse_file_type; a filter that does
# not match makes the native dialog raise instead of opening.
PYWEBVIEW_FILTER = r"^([\w ]+)\((\*(?:\.(?:\w+|\*))*(?:;\*(?:\.(?:\w+|\*))*)*)\)$"


def test_dialog_filters_are_valid_for_pywebview():
    for f in (IMPORT_FILTER, *EXPORT_FILTERS.values()):
        assert re.search(PYWEBVIEW_FILTER, f), f


def test_safe_filename():
    assert _safe_filename('Rapat: Q4/budget? "final"') == "Rapat Q4budget final"
    assert _safe_filename("...") == "Notulen"
    assert len(_safe_filename("x" * 300)) == 80
