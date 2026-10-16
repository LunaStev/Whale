import importlib.util
from pathlib import Path
import unittest


MODULE_SPEC = importlib.util.spec_from_file_location(
    "format_rust",
    Path(__file__).with_name("format-rust.py"),
)
FORMAT_RUST = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(FORMAT_RUST)


class ItemSpacingTests(unittest.TestCase):
    def test_attributes_and_comments_remain_with_the_next_function(self):
        source = "fn first() {}\n// next function\n#[inline]\nfn second() {}\n"

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "fn first() {}\n\n// next function\n#[inline]\nfn second() {}\n",
        )

    def test_spaces_between_rust_declarations(self):
        source = "enum First { A }\nstruct Second;\nfn third() {}\n"

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "enum First { A }\n\nstruct Second;\n\nfn third() {}\n",
        )

    def test_spaces_between_nested_impl_methods(self):
        source = "impl Example {\nfn first() {}\nfn second() {}\n}\n"

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "impl Example {\nfn first() {}\n\nfn second() {}\n}\n",
        )

    def test_ignores_braces_in_raw_strings(self):
        source = 'fn first() { let text = r##"{{{"##; }\nfn second() {}\n'

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            'fn first() { let text = r##"{{{"##; }\n\nfn second() {}\n',
        )

    def test_spaces_between_wave_items(self):
        source = "struct State {}\nfun first() {}\nexport(c) fun second() {}\n"

        self.assertEqual(
            FORMAT_RUST.add_item_spacing(source),
            "struct State {}\n\nfun first() {}\n\nexport(c) fun second() {}\n",
        )

    def test_does_not_treat_rust_variables_named_fun_as_items(self):
        source = "fn test() {\nlet fun = value;\nfun.blocks.push(value);\n}\n"

        self.assertEqual(FORMAT_RUST.add_item_spacing(source), source)


if __name__ == "__main__":
    unittest.main()