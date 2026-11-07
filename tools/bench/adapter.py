# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Owner implementation boundary. The offline CLI never imports a provider SDK.

Implement these protocols separately to run the preregistered experiment. Do not
put credentials in manifests. Preserve complete requests, responses and usage in
private artifacts, then export redacted records for review.
"""
from dataclasses import dataclass
from typing import Protocol


class TokenCounter(Protocol):
    """Optional local, model-specific tokenizer; identify its name and revision."""

    name: str

    def count(self, text: str) -> int: ...


@dataclass(frozen=True)
class ModelRequest:
    trial_id: str
    model: str
    prompt: str
    seed: int
    # The owner adapter must freeze system instructions, decoding, tool schemas,
    # output limits and provider cache settings as part of this configuration.
    configuration_id: str


@dataclass(frozen=True)
class Usage:
    input_tokens: int  # Entire request, INCLUDING cached input.
    cached_input_tokens: int | None  # None means unavailable, never zero-fill.
    output_tokens: int  # Include billed reasoning; avoid counting it twice.


@dataclass(frozen=True)
class ModelResponse:
    answer: str
    usage: Usage
    wall_seconds: float
    provider_request_id: str


class ModelAdapter(Protocol):
    """ADAPTER INTERFACE ONLY: no implementation, dispatch or paid calls here.

    For sessions, also record each model request and tool operation, accumulated
    history, errors, retries, compactions, cache configuration and final answer.
    Template trials use a fresh conversation and a single request, without tools.
    A future runner must explicitly opt into network/cost and implement the full
    protocol; generating pairs does not authorize or perform a model call.
    """

    def run(self, request: ModelRequest) -> ModelResponse: ...
