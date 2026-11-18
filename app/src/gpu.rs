//! Handing frames to termmd: dma-bufs it shows, and a staging buffer the
//! CPU draws into and the GPU copies from, with the barriers that hand each
//! image over. As termmd's surface example (app/examples/surface.rs) does.

use std::os::fd::AsFd;
use std::ptr;
use std::rc::Rc;

use termmd_surface::{BufferInfo, Connection, Request};
use termmd_vulkan::dmabuf::{ARGB8888, DmabufImage};
use termmd_vulkan::ffi::*;
use termmd_vulkan::{Device, check};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A dma-buf the terminal shows, and whether it still reads it.
pub struct Slot {
    pub image: DmabufImage,
    pub busy: bool,
}

impl Slot {
    /// A new dma-buf, sent to the terminal as `buffer`.
    pub fn new(
        device: &Rc<Device>,
        conn: &Connection,
        surface: u32,
        buffer: u32,
        (width, height): (u32, u32),
        mods: &[u64],
    ) -> Result<Slot> {
        let image = DmabufImage::export(device, width, height, ARGB8888, mods, VK_IMAGE_USAGE_TRANSFER_DST_BIT)?;
        let b = &image.buf;
        let info = BufferInfo {
            width,
            height,
            format: b.format.drm,
            modifier: b.modifier,
            stride: b.plane.stride as u32,
            offset: b.plane.offset as u32,
        };
        conn.send(&Request::Buffer { surface, buffer, info }, Some(b.fd.as_fd()))?;
        Ok(Slot { image, busy: false })
    }
}

/// A host-visible buffer the CPU draws a frame into.
pub struct Staging {
    buffer: VkBuffer,
    memory: VkDeviceMemory,
    ptr: *mut u8,
    size: u64,
    device: Rc<Device>,
}

impl Staging {
    pub fn new(device: &Rc<Device>, size: u64) -> Result<Staging> {
        let f = &device.fns;
        let info = VkBufferCreateInfo {
            sType: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            pNext: ptr::null(),
            flags: 0,
            size,
            usage: VK_BUFFER_USAGE_TRANSFER_SRC_BIT,
            sharingMode: VK_SHARING_MODE_EXCLUSIVE,
            queueFamilyIndexCount: 0,
            pQueueFamilyIndices: ptr::null(),
        };
        let mut s = Staging {
            buffer: VK_NULL_HANDLE,
            memory: VK_NULL_HANDLE,
            ptr: ptr::null_mut(),
            size,
            device: device.clone(),
        };
        // SAFETY: valid device and create info; `s` destroys it on drop.
        check("vkCreateBuffer", unsafe { (f.vkCreateBuffer)(device.raw, &info, ptr::null(), &mut s.buffer) })?;
        let mut req = VkMemoryRequirements::default();
        // SAFETY: a buffer of ours.
        unsafe { (f.vkGetBufferMemoryRequirements)(device.raw, s.buffer, &mut req) };
        let flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
        let index = device.memory_type(req.memoryTypeBits, flags).ok_or("no host-visible memory")?;
        let alloc = VkMemoryAllocateInfo {
            sType: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            pNext: ptr::null(),
            allocationSize: req.size,
            memoryTypeIndex: index,
        };
        // SAFETY: valid allocate info; `s` frees it on drop.
        check("vkAllocateMemory", unsafe { (f.vkAllocateMemory)(device.raw, &alloc, ptr::null(), &mut s.memory) })?;
        // SAFETY: fresh memory sized for the buffer.
        check("vkBindBufferMemory", unsafe { (f.vkBindBufferMemory)(device.raw, s.buffer, s.memory, 0) })?;
        let mut mapped = ptr::null_mut();
        // SAFETY: host-visible memory, mapped once for the buffer's life.
        check("vkMapMemory", unsafe { (f.vkMapMemory)(device.raw, s.memory, 0, VK_WHOLE_SIZE, 0, &mut mapped) })?;
        s.ptr = mapped.cast();
        Ok(s)
    }

    /// The mapping, as `w`×`h` BGRA pixels.
    pub fn pixels(&mut self, (w, h): (u32, u32)) -> &mut [u8] {
        let len = w as usize * h as usize * 4;
        assert!(len as u64 <= self.size);
        // SAFETY: in bounds of a live mapping; no GPU work reads it now
        // (each copy is waited for).
        unsafe { std::slice::from_raw_parts_mut(self.ptr, len) }
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let f = &self.device.fns;
        // SAFETY: no submitted work uses it (each copy is waited for);
        // freeing the memory unmaps it.
        unsafe {
            (f.vkDestroyBuffer)(self.device.raw, self.buffer, ptr::null());
            (f.vkFreeMemory)(self.device.raw, self.memory, ptr::null());
        }
    }
}

/// Copy the staged frame into `image` and hand it over to the terminal
/// (the "foreign" queue family), waiting until the GPU is done.
pub fn copy(device: &Device, staging: &Staging, image: &DmabufImage, (w, h): (u32, u32)) -> Result<()> {
    let family = device.info.graphics_family.ok_or("no graphics queue")?;
    let range = VkImageSubresourceRange {
        aspectMask: VK_IMAGE_ASPECT_COLOR_BIT,
        baseMipLevel: 0,
        levelCount: 1,
        baseArrayLayer: 0,
        layerCount: 1,
    };
    let barrier =
        |old, new, src_stage, src_access, dst_stage, dst_access, src_family, dst_family| VkImageMemoryBarrier2 {
            sType: VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2,
            pNext: ptr::null(),
            srcStageMask: src_stage,
            srcAccessMask: src_access,
            dstStageMask: dst_stage,
            dstAccessMask: dst_access,
            oldLayout: old,
            newLayout: new,
            srcQueueFamilyIndex: src_family,
            dstQueueFamilyIndex: dst_family,
            image: image.image,
            subresourceRange: range,
        };
    let dep = |b: &VkImageMemoryBarrier2| VkDependencyInfo {
        sType: VK_STRUCTURE_TYPE_DEPENDENCY_INFO,
        pNext: ptr::null(),
        dependencyFlags: 0,
        memoryBarrierCount: 0,
        pMemoryBarriers: ptr::null(),
        bufferMemoryBarrierCount: 0,
        pBufferMemoryBarriers: ptr::null(),
        imageMemoryBarrierCount: 1,
        pImageMemoryBarriers: b,
    };
    // Its old contents don't matter: every pixel is written.
    let to_dst = barrier(
        VK_IMAGE_LAYOUT_UNDEFINED,
        VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_PIPELINE_STAGE_2_NONE,
        VK_ACCESS_2_NONE,
        VK_PIPELINE_STAGE_2_COPY_BIT,
        VK_ACCESS_2_TRANSFER_WRITE_BIT,
        VK_QUEUE_FAMILY_IGNORED,
        VK_QUEUE_FAMILY_IGNORED,
    );
    let release = barrier(
        VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
        VK_IMAGE_LAYOUT_GENERAL,
        VK_PIPELINE_STAGE_2_COPY_BIT,
        VK_ACCESS_2_TRANSFER_WRITE_BIT,
        VK_PIPELINE_STAGE_2_NONE,
        VK_ACCESS_2_NONE,
        family,
        VK_QUEUE_FAMILY_FOREIGN_EXT,
    );
    let region = VkBufferImageCopy {
        bufferOffset: 0,
        bufferRowLength: 0,
        bufferImageHeight: 0,
        imageSubresource: VkImageSubresourceLayers {
            aspectMask: VK_IMAGE_ASPECT_COLOR_BIT,
            mipLevel: 0,
            baseArrayLayer: 0,
            layerCount: 1,
        },
        imageOffset: VkOffset3D { x: 0, y: 0, z: 0 },
        imageExtent: VkExtent3D { width: w, height: h, depth: 1 },
    };
    // A real app records into its own command buffers and paces with
    // fences; waiting each time keeps this short.
    device.submit_once(|f, cmd| {
        // SAFETY: recording into a command buffer that is recording; the
        // buffer and image are ours and outlive the submission, which
        // `submit_once` waits for.
        unsafe {
            (f.vkCmdPipelineBarrier2)(cmd, &dep(&to_dst));
            (f.vkCmdCopyBufferToImage)(
                cmd,
                staging.buffer,
                image.image,
                VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                1,
                &region,
            );
            (f.vkCmdPipelineBarrier2)(cmd, &dep(&release));
        }
    })?;
    Ok(())
}
