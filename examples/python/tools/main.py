import os
import json
from openai import OpenAI
from min_dotenv import hyd_env

# Load environment variables from .env file (same location as TypeScript example)
hyd_env('../../.env')

# Get API key from environment or use fallback
api_key = os.environ.get('HELICONE_CONTROL_PLANE_API_KEY', 'fake-api-key')

client = OpenAI(
    # Required by SDK, but AI gateway handles real auth
    base_url="http://localhost:8080/ai",
    api_key=api_key
)

tools = [{
    "type": "function",
    "function": {
        "name": "get_weather",
        "description": "Get current temperature for a given location.",
        "parameters": {
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "City and country e.g. Bogotá, Colombia"
                }
            },
            "required": [
                "location"
            ],
            "additionalProperties": False
        },
        "strict": True
    }
},
    {
    "type": "function",
    "function": {
        "name": "get_local_time",
        "description": "Get the current time in a given location.",
        "parameters": {
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "City and country e.g. New York, USA"
                }
            },
            "required": ["location"],
            "additionalProperties": False
        },
        "strict": True
    }
}]

# Mocked tools


def get_weather(location):
    return {"temperature": "22°C", "description": f"Sunny in {location}"}


def get_local_time(location):
    return {"time": "14:30", "timezone": "UTC+2", "location": location}


messages = [
    {
        "role": "user",
                "content": "What's the weather and current time in Tokyi?"
    }
]


def main():
    completion = client.chat.completions.create(
        model="openai/gpt-4o-mini",  # 100+ models available
        messages=messages,
        max_tokens=400,
        tools=tools,
        stream=True
    )

    tool_calls = completion.choices[0].message.tool_calls
    if tool_calls:
        messages.append({
            "role": "assistant",
            "content": completion.choices[0].message.content,
            "tool_calls": [tc.model_dump() for tc in tool_calls]
        })

        for tool_call in tool_calls:
            function_name = tool_call.function.name
            arguments = json.loads(tool_call.function.arguments)

            if function_name == "get_weather":
                result = get_weather(**arguments)
            elif function_name == "get_local_time":
                result = get_local_time(**arguments)

            messages.append({
                "role": "tool",
                "tool_call_id": tool_call.id,
                "name": function_name,
                "content": json.dumps(result)
            })

        followup = client.chat.completions.create(
            model="openai/gpt-4o-mini",
            messages=messages,
            tools=tools,
            stream=True,
            temperature=0.9,
        )

        for chunk in followup:
            print(chunk.choices[0].delta.content, end="", flush=True)


if __name__ == "__main__":
    main()
