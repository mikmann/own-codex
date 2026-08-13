import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("build_npm_package.py")
SPEC = importlib.util.spec_from_file_location("build_npm_package", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
build_npm_package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(build_npm_package)


class ConfigureSdkPackageJsonTest(unittest.TestCase):
    def test_sdk_depends_on_extended_cli_and_keeps_other_dependencies(self) -> None:
        package_json = {
            "scripts": {"prepare": "pnpm run build", "test": "jest"},
            "dependencies": {"existing": "1.0.0"},
        }

        build_npm_package.configure_sdk_package_json(package_json, "0.147.1-alpha-1")

        self.assertEqual(package_json["scripts"], {"test": "jest"})
        self.assertEqual(
            package_json["dependencies"],
            {
                "existing": "1.0.0",
                "codex-extended-workflow": "0.147.1-alpha-1",
            },
        )


if __name__ == "__main__":
    unittest.main()
