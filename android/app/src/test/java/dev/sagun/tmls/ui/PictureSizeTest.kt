package dev.sagun.tmls.ui

import org.junit.Assert.assertEquals
import org.junit.Test

class PictureSizeTest {
    @Test fun aSmallPictureIsNotBlownUp() {
        assertEquals(96f to 96f, pictureSize(192f, 192f, 2f, 300f))
        assertEquals(64f to 64f, pictureSize(64f, 64f, 1f, 300f))
    }

    @Test fun tinyOnesGrowToTheShortSideMinimum() {
        assertEquals(48f to 48f, pictureSize(16f, 16f, 1f, 300f))
        assertEquals(48f to 48f, pictureSize(64f, 64f, 3f, 300f))
    }

    @Test fun bigOnesFitTheWidthAndHeightKeepingTheRatio() {
        assertEquals(300f to 150f, pictureSize(1200f, 600f, 1f, 300f))
        assertEquals(180f to 360f, pictureSize(500f, 1000f, 1f, 300f))
    }
}
