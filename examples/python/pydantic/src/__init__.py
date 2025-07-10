import os
from typing import Any

import httpx
from min_dotenv import hyd_env
from pydantic_ai.models.anthropic import AnthropicModel
from pydantic_ai.providers.anthropic import AnthropicProvider

# Load environment variables from .env file (same location as other examples)
hyd_env('../../../.env')

# API key configuration
ANTHROPIC_API_KEY = os.environ.get('ANTHROPIC_API_KEY')
HELICONE_API_KEY = os.environ.get('HELICONE_API_KEY')


def get_anthropic_model(model_name: str = "claude-3-5-sonnet-20241022") -> AnthropicModel:
    """
    Get an Anthropic model with Helicone automatically configured if available.

    Args:
        model_name: The Anthropic model name to use

    Returns:
        AnthropicModel configured with Helicone if API key is available
    """
    provider_kwargs: dict[str, Any] = {"api_key": ANTHROPIC_API_KEY}

    if HELICONE_API_KEY:
        print("Using Helicone")

    # Add Helicone configuration if API key is available
    provider_kwargs["http_client"] = httpx.AsyncClient(
        base_url="https://anthropic.helicone.ai",
        headers={
            "Helicone-Auth": f"Bearer {HELICONE_API_KEY}",
            "Helicone-Cache-Enabled": "true",
            "Helicone-Cache-Bucket-Max-Size": "1",
        }
    )
    
    provider = AnthropicProvider(**provider_kwargs)
    return AnthropicModel(model_name, provider=provider)


# Default model for backward compatibility
model = get_anthropic_model() 