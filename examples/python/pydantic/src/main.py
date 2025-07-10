import asyncio
import logging

from pydantic_ai import Agent
from . import get_anthropic_model

# Configure logging
logging.basicConfig(level=logging.INFO)

# Enable httpx logging to see actual HTTP requests
logging.getLogger("httpx").setLevel(logging.INFO)

async def main():
    print("Hello, World!")

    # Get the Anthropic model with Helicone configuration
    model = get_anthropic_model("claude-3-5-sonnet-20241022")  # Using a real available model

    # Create a simple chat completion
    messages = [
        {
            "role": "system", 
            "content": "You are a helpful assistant that can answer questions and help with tasks."
        },
        {
            "role": "user",
            "content": "Hello, world!"
        }
    ]

    agent = Agent(
        model=model, 
        system_prompt="You are a helpful assistant that can answer questions and help with tasks.",
        model_settings={'max_tokens': 400}
    )

    try:
        # Use the agent to get a response
        result = await agent.run("Hello, world!")
        print(result.output)
    except Exception as e:
        print(f"Error: {e}")


if __name__ == "__main__":
    asyncio.run(main()) 