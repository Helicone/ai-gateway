import os
from openai import OpenAI
from min_dotenv import hyd_env

# Load environment variables from .env file (same location as TypeScript example)
hyd_env('../../../.env')

# Get API key from environment or use fallback
api_key = os.environ.get('HELICONE_CONTROL_PLANE_API_KEY', 'fake-api-key')

client = OpenAI(
    # Required by SDK, but AI gateway handles real auth
    base_url="http://localhost:8080/ai",
    api_key=api_key
)


def main():
    print("Hello, World!")

    completion = client.chat.completions.create(
        model="openai/gpt-4o-mini",  # 100+ models available
        messages=[
            {
                "role": "system",
                "content": "You are a helpful assistant that can answer questions and help with tasks."
            },
            {
                "role": "user",
                "content": "Hello, world!"
            }
        ],
        max_tokens=400,
    )

    print(completion.choices[0].message.content)
    # for chunk in completion:
        # print(chunk)


if __name__ == "__main__":
    main()