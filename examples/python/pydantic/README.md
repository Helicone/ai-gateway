# Python pydantic_ai Example with Helicone

This example demonstrates how to use pydantic_ai with Anthropic models through Helicone for monitoring and caching.

## Setup

1. Install uv if you haven't already:
   ```bash
   curl -LsSf https://astral.sh/uv/install.sh | sh
   ```

2. Install dependencies using uv:
   ```bash
   uv sync
   ```

3. Set up environment variables in `.env` file (at the root of the ai-gateway project):
   ```env
   ANTHROPIC_API_KEY=your_anthropic_api_key_here
   HELICONE_API_KEY=your_helicone_api_key_here  # Optional but recommended
   ```

## Running the Example

```bash
uv run python -m src.main
```

## Structure

This example is simplified to use only 2 files:
- `src/__init__.py` - Contains all configuration and model setup
- `src/main.py` - Contains the main application logic

## Features

- **pydantic_ai Integration**: Uses pydantic_ai for type-safe AI interactions
- **Anthropic Models**: Supports Claude models through Anthropic's API
- **Helicone Monitoring**: Automatically configures Helicone for request monitoring and caching when API key is provided
- **Async Support**: Uses async/await for better performance
- **Configurable Models**: Easy to switch between different Claude models

## Configuration

The example automatically configures Helicone when a `HELICONE_API_KEY` is provided:
- Enables caching for faster responses
- Provides request monitoring and analytics
- Uses Anthropic-specific Helicone endpoint

If no Helicone API key is provided, the example will still work with direct Anthropic API calls. 