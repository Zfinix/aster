"""Harbor agent that runs the released aster binary inside each task container.

Run from the repo root:
    PYTHONPATH=crates/aster-eval/harbor harbor run -a aster_agent:Aster ...
Model and credentials come in as ASTER_* variables through --ae. Pass
--ak binary=PATH to test a local static Linux build instead of the release.
"""

import json
import shlex
from pathlib import Path
from typing import override

from harbor.agents.installed.base import BaseInstalledAgent, with_prompt_template
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

_REPO = "Zfinix/aster"
_STREAM = "aster.ndjson"
_STDERR = "aster.stderr"


class Aster(BaseInstalledAgent):
    def __init__(self, *args, binary: str | None = None, **kwargs):
        super().__init__(*args, **kwargs)
        self._binary = Path(binary).expanduser() if binary else None

    @staticmethod
    @override
    def name() -> str:
        return "aster"

    @override
    def get_version_command(self) -> str | None:
        return "aster --version"

    @override
    def parse_version(self, stdout: str) -> str:
        return stdout.strip().splitlines()[-1].split()[-1]

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        if self._binary:
            await environment.upload_file(self._binary, "/usr/local/bin/aster")
            await self.exec_as_root(
                environment, command="chmod 755 /usr/local/bin/aster && aster --version"
            )
            return
        await self.ensure_system_dependencies(environment, ("curl", "bash"))
        # The static musl build runs on any task image, whatever its libc.
        version = shlex.quote(self._version or "")
        await self.exec_as_root(
            environment,
            command=(
                "set -euo pipefail; "
                f"v={version}; "
                'if [ -z "$v" ]; then '
                f"v=$(curl -fsSL https://api.github.com/repos/{_REPO}/releases/latest "
                "| sed -n 's/.*\"tag_name\": *\"cli-v\\([^\"]*\\)\".*/\\1/p'); fi; "
                "arch=$(uname -m); [ \"$arch\" = arm64 ] && arch=aarch64; "
                f"curl -fsSL https://github.com/{_REPO}/releases/download/cli-v$v/"
                "aster-$v-$arch-unknown-linux-musl.tar.gz "
                "| tar -xz -C /tmp && "
                "install -m 755 $(find /tmp -maxdepth 2 -type f -name aster | head -1) "
                "/usr/local/bin/aster && aster --version"
            ),
        )

    @override
    @with_prompt_template
    async def run(
        self,
        instruction: str,
        environment: BaseEnvironment,
        context: AgentContext,
    ) -> None:
        # The container is the isolation, so yolo skips aster's own sandbox and
        # approvals; there is nobody to answer them.
        await self.exec_as_agent(
            environment,
            command=(
                "set -o pipefail; "
                f"aster --stream --permission-mode yolo {shlex.quote(instruction)} "
                f"</dev/null 2>/logs/agent/{_STDERR} "
                f"| grep --line-buffered -v -E '\"type\":\"(token|reasoning_delta)\"' "
                f"| tee /logs/agent/{_STREAM}"
            ),
        )

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        stream = self.logs_dir / _STREAM
        if not stream.exists():
            return
        for line in stream.read_text().splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            # A run that stopped on an error still spent tokens, and says so.
            if event.get("type") not in ("done", "error"):
                continue
            usage = event.get("usage") or {}
            context.n_input_tokens = usage.get("prompt_tokens")
            context.n_output_tokens = usage.get("completion_tokens")
            context.cost_usd = usage.get("estimated_cost_usd")
