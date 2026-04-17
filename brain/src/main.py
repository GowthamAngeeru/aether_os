import asyncio
import logging
import os
import sys
import grpc
from dotenv import load_dotenv

sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

import aether_pb2_grpc
from services.rag_services import RagServices

load_dotenv()

logging.basicConfig(
    level=getattr(logging, os.getenv("LOG_LEVEL", "INFO")),
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s"
)
logger = logging.getLogger(__name__)

async def serve():
    port = os.getenv("PORT", "50051")

    server=grpc.aio.server()
    aether_pb2_grpc.add_RagServiceServicer_to_server(RagServices(), server)
    
    server.add_insecure_port(f"[::]:{port}")

    logger.info(f"🧠 Python Brain gRPC Server starting on port {port}...")
    await server.start()

    await server.wait_for_termination()

if __name__ == "__main__":
    try:
        asyncio.run(serve())
    except KeyboardInterrupt:
        logger.info("Server shutting down...")
