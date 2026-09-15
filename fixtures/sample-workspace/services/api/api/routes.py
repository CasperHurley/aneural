from api import models
from fastapi import APIRouter

router = APIRouter()

# FIXME: pagination
@router.get("/items")
def items():
    return models.all_items()
